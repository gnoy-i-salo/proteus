use super::*;
use crate::corpus_database::{JobBudget, JobProvider, RecordNamespace};
use crate::ingestion::{
    preview_store::read_preview,
    preview_workflow::{preview, PreviewRequest},
    SelectedInputs,
};
fn budget() -> JobBudget {
    JobBudget {
        max_input_bytes: 1 << 20,
        max_expanded_bytes: 1 << 20,
        max_documents: 100,
        max_batch_documents: 10,
        max_batch_bytes: 1 << 20,
        max_duration_ms: 60_000,
    }
}
fn selected(dir: &std::path::Path, text: &str) -> SelectedInputs {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("selected.json");
    std::fs::write(&path, text).unwrap();
    let inputs = SelectedInputs::default();
    inputs.insert("native-handle".into(), &path).unwrap();
    inputs
}
fn request() -> PreviewRequest {
    PreviewRequest {
        operation_id: "preview".into(),
        source_handle: "native-handle".into(),
        budget: budget(),
    }
}
#[test]
fn selected_archive_preview_persists_verified_metadata_without_exposing_its_path() {
    let (sources, dir) = local();
    let raw = r#"{"name":"  Чат é\r\n","type":"personal_chat","id":1,"messages":[{"id":1,"type":"message","from_id":"user1","text":"  exact text"}]}"#;
    let inputs = selected(&dir, raw);
    let operation = sources
        .begin_ingestion_operation("preview", JobProvider::Telegram)
        .unwrap();
    let result = preview(&sources, &inputs, &operation, request()).unwrap();
    assert_eq!(result.schema_version, 1);
    let acquired = inputs
        .read_preview(&result.id, |file, origin| {
            use std::io::Read;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            Ok((bytes, origin.to_string()))
        })
        .unwrap();
    assert_eq!(acquired.0, raw.as_bytes());
    assert!(acquired.1.starts_with("localArchivePath:1:sha256:"));
    assert_eq!(result.source_handle, "native-handle");
    assert_eq!(result.resources[0].label, "  Чат é\r\n");
    assert_eq!(result.resources[0].author_ids, ["user1"]);
    assert_eq!(result.resources[0].estimated_documents, Some(1));
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains(dir.to_str().unwrap()));
    sources
        .with_ingestion_database(&operation, |db| {
            let stored = read_preview(db, &result.id)?;
            assert_eq!(stored.schema_version, 2);
            assert_eq!(stored.fingerprint.byte_length, raw.len() as u64);
            assert_eq!(stored.resources, result.resources);
            Ok(())
        })
        .unwrap();
    drop(operation);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn invalid_budgets_handles_or_archive_tails_never_persist_partial_previews() {
    for case in 0..4 {
        let (sources, dir) = local();
        let inputs = selected(
            &dir,
            if case == 3 {
                "{\"name\":\"chat\",\"type\":\"personal_chat\",\"id\":1,\"messages\":[]"
            } else {
                "{\"name\":\"chat\",\"type\":\"personal_chat\",\"id\":1,\"messages\":[]}"
            },
        );
        let operation = sources
            .begin_ingestion_operation("preview", JobProvider::Telegram)
            .unwrap();
        let mut request = request();
        match case {
            0 => request.source_handle = dir.join("selected.json").to_string_lossy().into(),
            1 => request.budget.max_input_bytes = 1,
            2 => request.budget.max_documents = -1,
            _ => {}
        }
        assert!(preview(&sources, &inputs, &operation, request).is_err());
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
fn cancelled_native_preview_never_opens_a_file_or_persists_metadata() {
    let (sources, dir) = local();
    let operation = sources
        .begin_ingestion_operation("preview", JobProvider::Telegram)
        .unwrap();
    sources.cancel_ingestion_operation("preview").unwrap();
    let error = preview(&sources, &SelectedInputs::default(), &operation, request())
        .err()
        .unwrap();
    assert_eq!(error, "Операция отменена.");
    let fresh = sources
        .begin_ingestion_operation("inspect", JobProvider::Telegram)
        .unwrap();
    sources
        .with_ingestion_database(&fresh, |db| {
            assert!(db.ids_in_namespace(RecordNamespace::Ingestion)?.is_empty());
            Ok(())
        })
        .unwrap();
    drop(fresh);
    drop(operation);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn preview_wire_accepts_only_explicit_camel_case_budget_and_capability_fields() {
    let value = serde_json::json!({"operationId":"preview","sourceHandle":"native-handle","budget":budget()});
    assert!(serde_json::from_value::<PreviewRequest>(value.clone()).is_ok());
    for field in ["path", "provider", "grantId", "source-handle"] {
        let mut forged = value.clone();
        forged[field] = serde_json::json!("forged");
        assert!(serde_json::from_value::<PreviewRequest>(forged).is_err());
    }
}

#[test]
fn full_native_receipt_cache_does_not_leave_an_unusable_persisted_preview() {
    let (sources, dir) = local();
    let inputs = selected(
        &dir,
        r#"{"name":"chat","type":"personal_chat","id":1,"messages":[]}"#,
    );
    let operation = sources
        .begin_ingestion_operation("preview", JobProvider::Telegram)
        .unwrap();
    let first = preview(&sources, &inputs, &operation, request()).unwrap();
    inputs.forget_preview(&first.id).unwrap();
    for i in 0..16 {
        inputs
            .associate_preview("native-handle", &format!("receipt-{i}"))
            .unwrap();
    }
    assert!(preview(&sources, &inputs, &operation, request()).is_err());
    sources
        .with_ingestion_database(&operation, |db| {
            assert_eq!(db.ids_in_namespace(RecordNamespace::Ingestion)?, [first.id]);
            Ok(())
        })
        .unwrap();
    drop(operation);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
