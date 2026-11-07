use super::{operations::Operation, preview::PreviewResource, SelectedInputs};
use crate::{corpus_database::JobBudget, sources::Sources};
use serde::{Deserialize, Serialize};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewRequest {
    pub operation_id: String,
    pub source_handle: String,
    pub budget: JobBudget,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResult {
    pub schema_version: u8,
    pub id: String,
    pub source_handle: String,
    pub source_format: String,
    pub adapter_version: String,
    pub resources: Vec<PreviewResource>,
    pub limitations: Vec<String>,
}
pub fn preview(
    sources: &Sources,
    inputs: &SelectedInputs,
    operation: &Operation,
    request: PreviewRequest,
) -> Result<PreviewResult, String> {
    crate::corpus_database::validate_ingestion_budget(&request.budget)?;
    operation.check()?;
    let limits = super::telegram_archive_json::TelegramArchiveLimits {
        max_input_bytes: request.budget.max_input_bytes as u64,
        max_message_bytes: request.budget.max_batch_bytes.min(4 * 1024 * 1024) as usize,
        max_metadata_bytes: request.budget.max_batch_bytes.min(65536) as usize,
        max_messages: request.budget.max_documents as u64,
        max_duration: std::time::Duration::from_millis(request.budget.max_duration_ms as u64),
    };
    let metadata_budget = request.budget.max_expanded_bytes.min(32 * 1024 * 1024) as usize;
    let scanned = inputs.read(&request.source_handle, |reader| {
        super::preview_scan::scan_telegram_preview(
            reader,
            &limits,
            metadata_budget,
            operation.cancelled(),
        )
    })?;
    let stored = super::preview_store::issue_preview(
        "telegram".into(),
        "telegramDesktopJson".into(),
        "telegramDesktopJson:1".into(),
        scanned.fingerprint,
        scanned.resources,
    )?;
    // Never acquire the input registry while holding the database gate: streaming
    // ingestion may hold an input descriptor while committing a bounded batch.
    persist_preview(inputs, &request.source_handle, &stored.id, || {
        sources.with_ingestion_database(operation, |db| {
            super::preview_store::save_preview(db, &stored)
        })
    })?;
    Ok(PreviewResult {
        schema_version: 1, id: stored.id, source_handle: request.source_handle,
        source_format: stored.source_format, adapter_version: stored.adapter_version,
        resources: stored.resources,
        limitations: vec!["Предпросмотр содержит метаданные выбранного архива Telegram. Сообщения ещё не загружены в корпус и не изучены моделью.".into()],
    })
}

fn persist_preview(
    inputs: &SelectedInputs,
    handle: &str,
    preview_id: &str,
    save: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    inputs.associate_preview(handle, preview_id)?;
    match save() {
        Ok(()) => Ok(()),
        Err(error) => {
            inputs.forget_preview(preview_id)?;
            Err(error)
        }
    }
}
#[cfg(test)]
mod persistence_tests {
    use super::*;
    #[test]
    fn failed_persistence_releases_the_native_receipt_slot_for_a_retry() {
        let path = std::env::temp_dir().join(format!(
            "proteus-preview-persist-{}.json",
            rand::random::<u64>()
        ));
        std::fs::write(&path, "{}").unwrap();
        let inputs = SelectedInputs::default();
        inputs.insert("selected".into(), &path).unwrap();
        for i in 0..32 {
            assert_eq!(
                persist_preview(&inputs, "selected", &format!("receipt-{i}"), || Err(
                    "disk failure".into()
                ))
                .unwrap_err(),
                "disk failure"
            );
        }
        persist_preview(&inputs, "selected", "success", || Ok(())).unwrap();
        assert!(inputs.read_preview("success", |_, _| Ok(())).is_ok());
        drop(inputs);
        std::fs::remove_file(path).unwrap();
    }
}
