use super::*;
use crate::corpus_database::{JobBudget, JobProvider, RecordNamespace};
use crate::ingestion::{
    page_preview::URL_ADAPTER_VERSION,
    preview_store::read_preview,
    selected_page::PageSnapshot,
    url_preview::{preview_with, UrlPreviewRequest},
    SelectedInputs,
};
use sha2::{Digest, Sha256};
const RAW: &str = "<title> Точное имя 🦉</title><p>  слова\r\n</p>";
fn request() -> UrlPreviewRequest {
    UrlPreviewRequest {
        operation_id: "url-preview".into(),
        selected_url: "https://example.com/article#part".into(),
        budget: JobBudget {
            max_input_bytes: 1 << 20,
            max_expanded_bytes: 1 << 20,
            max_documents: 1,
            max_batch_documents: 1,
            max_batch_bytes: 1 << 20,
            max_duration_ms: 60_000,
        },
    }
}
fn snapshot() -> PageSnapshot {
    PageSnapshot {
        selected_url: request().selected_url,
        final_url: "https://example.com/article".into(),
        redirects: vec![],
        content_type: "text/html".into(),
        original: RAW.into(),
        sha256: format!("{:x}", Sha256::digest(RAW)),
        retrieved_at: "2026-11-08T09:00:00Z".into(),
    }
}
#[test]
fn selected_url_preview_retains_exact_native_original_and_only_persists_metadata() {
    let (sources, dir) = local();
    let inputs = SelectedInputs::default();
    let operation = sources
        .begin_ingestion_operation("url-preview", JobProvider::Web)
        .unwrap();
    let result = preview_with(
        &sources,
        &inputs,
        &operation,
        request(),
        |url, limits, check| {
            assert_eq!(url, request().selected_url);
            assert_eq!(limits.max_bytes, 1 << 20);
            check()?;
            Ok(snapshot())
        },
    )
    .unwrap();
    assert_eq!(result.adapter_version, URL_ADAPTER_VERSION);
    assert_eq!(result.source_format, "selectedUrlHtml");
    assert_eq!(result.resources[0].label, " Точное имя 🦉");
    assert!(result.resources[0].author_ids.is_empty());
    inputs
        .urls
        .read_preview(&result.id, |input| {
            assert_eq!(input.original, RAW);
            Ok(())
        })
        .unwrap();
    sources
        .with_ingestion_database(&operation, |db| {
            let stored = read_preview(db, &result.id)?;
            assert_eq!(stored.schema_version, 4);
            assert_eq!(stored.provider, "web");
            assert_eq!(stored.fingerprint.sha256, snapshot().sha256);
            let record = db.read(&result.id)?.unwrap();
            assert!(record.input.documents.is_empty());
            assert!(!record.input.state_json.contains("слова"));
            Ok(())
        })
        .unwrap();
    inputs.release(&result.source_handle).unwrap();
    assert!(inputs.urls.read_preview(&result.id, |_| Ok(())).is_err());
    drop(operation);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn invalid_url_preview_never_fetches_or_persists_or_exhausts_registry() {
    let (sources, dir) = local();
    let inputs = SelectedInputs::default();
    let operation = sources
        .begin_ingestion_operation("url-preview", JobProvider::Web)
        .unwrap();
    for case in 0..5 {
        let mut req = request();
        match case {
            0 => req.operation_id = "other".into(),
            1 => req.budget.max_duration_ms = 0,
            2 => req.budget.max_input_bytes = 0,
            3 => req.selected_url = "http://127.0.0.1/private".into(),
            _ => req.selected_url = "file:///secret".into(),
        }
        assert!(
            preview_with(&sources, &inputs, &operation, req, |_, _, _| panic!(
                "invalid request reached transport"
            ))
            .is_err()
        );
    }
    for _ in 0..12 {
        assert!(
            preview_with(&sources, &inputs, &operation, request(), |_, _, _| {
                let mut bad = snapshot();
                bad.sha256 = "a".repeat(64);
                Ok(bad)
            })
            .is_err()
        );
    }
    sources
        .with_ingestion_database(&operation, |db| {
            assert!(db.ids_in_namespace(RecordNamespace::Ingestion)?.is_empty());
            Ok(())
        })
        .unwrap();
    let good = preview_with(&sources, &inputs, &operation, request(), |_, _, _| {
        Ok(snapshot())
    })
    .unwrap();
    inputs.forget_preview(&good.id).unwrap();
    assert!(inputs.urls.read_preview(&good.id, |_| Ok(())).is_err());
    assert!(inputs.urls.read(&good.source_handle, |_| Ok(())).is_ok());
    drop(operation);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn cancellation_after_url_acquisition_publishes_nothing() {
    let (sources, dir) = local();
    let inputs = SelectedInputs::default();
    let operation = sources
        .begin_ingestion_operation("url-preview", JobProvider::Web)
        .unwrap();
    let result = preview_with(&sources, &inputs, &operation, request(), |_, _, _| {
        sources.cancel_ingestion_operation("url-preview")?;
        Ok(snapshot())
    });
    assert_eq!(result.err().as_deref(), Some("Операция отменена."));
    drop(operation);
    let inspection = sources
        .begin_ingestion_operation("inspection", JobProvider::Web)
        .unwrap();
    sources
        .with_ingestion_database(&inspection, |db| {
            assert!(db.ids_in_namespace(RecordNamespace::Ingestion)?.is_empty());
            Ok(())
        })
        .unwrap();
    drop(inspection);
    drop(inputs);
    drop(sources);
    if dir.exists() {
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn selected_url_request_rejects_injected_origins_and_authority() {
    let original = serde_json::json!({"operationId":"url-preview","selectedUrl":request().selected_url,"budget":request().budget});
    assert!(serde_json::from_value::<UrlPreviewRequest>(original.clone()).is_ok());
    for field in [
        "original",
        "sha256",
        "origin",
        "grantId",
        "sourceHandle",
        "selected_url",
        "provider",
    ] {
        let mut invalid = original.clone();
        invalid[field] = serde_json::json!("forged");
        assert!(serde_json::from_value::<UrlPreviewRequest>(invalid).is_err());
    }
}
#[test]
fn selected_url_preview_rejects_provider_mismatch_and_normalized_budget_overflow() {
    let (sources, dir) = local();
    let inputs = SelectedInputs::default();
    let wrong = sources
        .begin_ingestion_operation("url-preview", JobProvider::SavedPage)
        .unwrap();
    assert!(
        preview_with(&sources, &inputs, &wrong, request(), |_, _, _| panic!(
            "wrong provider fetched"
        ))
        .is_err()
    );
    drop(wrong);
    let operation = sources
        .begin_ingestion_operation("url-preview", JobProvider::Web)
        .unwrap();
    let mut tiny = request();
    tiny.budget.max_batch_bytes = RAW.len() as i64;
    assert!(preview_with(
        &sources,
        &inputs,
        &operation,
        tiny,
        |_, _, _| Ok(snapshot())
    )
    .is_err());
    sources
        .with_ingestion_database(&operation, |db| {
            assert!(db.ids_in_namespace(RecordNamespace::Ingestion)?.is_empty());
            Ok(())
        })
        .unwrap();
    drop(operation);
    drop(inputs);
    drop(sources);
    if dir.exists() {
        std::fs::remove_dir_all(dir).unwrap();
    }
}
