use super::*;
use crate::corpus_database::{JobBudget, JobProvider, RecordNamespace};
use crate::ingestion::{
    page_preview::{preview_saved, PagePreviewRequest, ADAPTER_VERSION},
    preview_store::read_preview,
    SelectedInputs,
};
use sha2::{Digest, Sha256};
fn request() -> PagePreviewRequest {
    PagePreviewRequest {
        operation_id: "page-preview".into(),
        source_handle: "native-page".into(),
        content_type: "text/html".into(),
        budget: JobBudget {
            max_input_bytes: 1 << 20,
            max_expanded_bytes: 1 << 20,
            max_documents: 10,
            max_batch_documents: 2,
            max_batch_bytes: 1 << 20,
            max_duration_ms: 60_000,
        },
    }
}
fn select(dir: &std::path::Path, bytes: &[u8]) -> SelectedInputs {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("  Источник 🦉.html");
    std::fs::write(&path, bytes).unwrap();
    let inputs = SelectedInputs::default();
    inputs.insert("native-page".into(), &path).unwrap();
    inputs
}
#[test]
fn saved_page_preview_records_exact_fingerprint_but_no_body_or_invented_author() {
    let (sources, dir) = local();
    let raw = "<title>  Точное имя 🦉</title><meta name='author' content='Reported label'><p> Слова\r\n</p>";
    let inputs = select(&dir, raw.as_bytes());
    let operation = sources
        .begin_ingestion_operation("page-preview", JobProvider::SavedPage)
        .unwrap();
    let result = preview_saved(&sources, &inputs, &operation, request()).unwrap();
    assert_eq!(result.schema_version, 1);
    assert_eq!(result.adapter_version, ADAPTER_VERSION);
    assert_eq!(result.source_format, "savedPageHtml");
    assert_eq!(result.source_handle, "native-page");
    assert_eq!(result.resources.len(), 1);
    assert_eq!(result.resources[0].label, "  Точное имя 🦉");
    assert!(result.resources[0]
        .id
        .starts_with("pageSource:1:savedPage:"));
    assert!(result.resources[0].author_ids.is_empty());
    assert_eq!(result.resources[0].estimated_documents, Some(1));
    let wire = serde_json::to_string(&result).unwrap();
    assert!(
        !wire.contains("Reported label")
            && !wire.contains("Слова")
            && !wire.contains(dir.to_str().unwrap())
    );
    inputs
        .read_preview(&result.id, |_, origin| {
            assert!(origin.starts_with("localArchivePath:1:sha256:"));
            Ok(())
        })
        .unwrap();
    sources
        .with_ingestion_database(&operation, |db| {
            let stored = read_preview(db, &result.id)?;
            assert_eq!(stored.schema_version, 3);
            assert!(matches!(
                stored.origin,
                Some(crate::ingestion::page_document::PageOrigin::SavedPage { .. })
            ));
            assert_eq!(stored.provider, "savedPage");
            assert_eq!(stored.fingerprint.byte_length, raw.len() as u64);
            assert_eq!(
                stored.fingerprint.sha256,
                format!("{:x}", Sha256::digest(raw.as_bytes()))
            );
            let record = db.read(&result.id)?.unwrap();
            assert!(record.input.documents.is_empty());
            assert!(!record.input.state_json.contains("Слова"));
            Ok(())
        })
        .unwrap();
    drop(operation);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn plain_and_markdown_previews_keep_the_exact_native_filename_without_fake_dates() {
    for (content_type, format) in [
        ("text/plain", "savedPageText"),
        ("text/markdown", "savedPageMarkdown"),
    ] {
        let (sources, dir) = local();
        let inputs = select(&dir, "  точный текст\r\n".as_bytes());
        let operation = sources
            .begin_ingestion_operation("page-preview", JobProvider::SavedPage)
            .unwrap();
        let mut req = request();
        req.content_type = content_type.into();
        let result = preview_saved(&sources, &inputs, &operation, req).unwrap();
        assert_eq!(result.source_format, format);
        assert_eq!(result.resources[0].label, "  Источник 🦉.html");
        assert!(result.resources[0].author_ids.is_empty());
        drop(operation);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn invalid_page_inputs_never_leave_partial_preview_receipts() {
    for case in 0..9 {
        let (sources, dir) = local();
        let inputs = select(&dir, if case == 0 { &[0xff] } else { b"<p>words</p>" });
        let operation = sources
            .begin_ingestion_operation(
                "page-preview",
                if case == 7 {
                    JobProvider::Telegram
                } else {
                    JobProvider::SavedPage
                },
            )
            .unwrap();
        let mut req = request();
        match case {
            1 => req.budget.max_input_bytes = 2,
            2 => req.content_type = "application/json".into(),
            3 => req.source_handle = dir.to_string_lossy().into_owned(),
            4 => req.operation_id = "different-operation".into(),
            5 => req.budget.max_expanded_bytes = 1,
            6 => req.budget.max_duration_ms = 0,
            8 => req.budget.max_batch_bytes = 128,
            _ => {}
        }
        assert!(
            preview_saved(&sources, &inputs, &operation, req).is_err(),
            "case {case}"
        );
        sources
            .with_ingestion_database(&operation, |db| {
                assert!(db.ids_in_namespace(RecordNamespace::Ingestion)?.is_empty());
                Ok(())
            })
            .unwrap();
        drop(operation);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn cancelled_saved_preview_checks_before_reading_a_missing_capability() {
    let (sources, dir) = local();
    let operation = sources
        .begin_ingestion_operation("page-preview", JobProvider::SavedPage)
        .unwrap();
    sources.cancel_ingestion_operation("page-preview").unwrap();
    assert_eq!(
        preview_saved(&sources, &SelectedInputs::default(), &operation, request())
            .err()
            .as_deref(),
        Some("Операция отменена.")
    );
    drop(operation);
    drop(sources);
    if dir.exists() {
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn page_preview_wire_rejects_paths_authority_and_aliases() {
    let original = serde_json::json!({"operationId":"page-preview","sourceHandle":"native-page","contentType":"text/plain","budget":request().budget});
    assert!(serde_json::from_value::<PagePreviewRequest>(original.clone()).is_ok());
    for key in [
        "path",
        "origin",
        "label",
        "provider",
        "grantId",
        "content_type",
    ] {
        let mut invalid = original.clone();
        invalid[key] = serde_json::json!("forged");
        assert!(
            serde_json::from_value::<PagePreviewRequest>(invalid).is_err(),
            "{key}"
        );
    }
}
