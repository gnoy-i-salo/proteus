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
    sources.with_ingestion_database(operation, |db| {
        super::preview_store::save_preview(db, &stored)
    })?;
    Ok(PreviewResult {
        schema_version: 1, id: stored.id, source_handle: request.source_handle,
        source_format: stored.source_format, adapter_version: stored.adapter_version,
        resources: stored.resources,
        limitations: vec!["Предпросмотр содержит метаданные выбранного архива Telegram. Сообщения ещё не загружены в корпус и не изучены моделью.".into()],
    })
}
