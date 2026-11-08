use super::{operations::Operation, SelectedInputs};
use crate::{corpus_database::StoredJob, sources::Sources};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreRequest {
    pub operation_id: String,
    pub job_id: String,
    pub source_handle: String,
}
pub fn restore(
    sources: &Sources,
    inputs: &SelectedInputs,
    operation: &Operation,
    request: RestoreRequest,
    now_ms: impl Fn() -> i64,
) -> Result<StoredJob, String> {
    if request.operation_id != operation.id() {
        return Err("Идентификатор восстановления не соответствует операции.".into());
    }
    let (job, preview) = sources.with_ingestion_database(operation, |db| {
        let job = super::job_workflow::read_job(db, &request.job_id)?;
        validate(db, &job, now_ms())?;
        let preview = super::preview_store::read_preview(db, &job.state.selection.preview_id)?;
        if super::adapter::Adapter::resolve(&preview)?.provider() != operation.provider() {
            return Err("Источник принадлежит другому сервису.".into());
        }
        Ok((job, preview))
    })?;
    if matches!(
        super::adapter::Adapter::resolve(&preview)?,
        super::adapter::Adapter::SelectedUrl
    ) {
        return restore_url(sources, inputs, operation, &request, &job, &preview, now_ms);
    }
    let saved_origin = restore_origin(&job, &preview, now_ms())?;
    // Input -> database is the same lock order as acquisition. Never reopen a
    // persisted path; only the freshly selected native capability supplies bytes.
    inputs.restore_checked_preview(
        &request.source_handle,
        &preview.id,
        |file, current_origin| {
            let mut reader = super::fingerprint::FingerprintReader::new(
                file,
                job.state.budget.max_input_bytes as u64,
                std::time::Duration::from_millis(job.state.budget.max_duration_ms as u64),
                operation.cancelled(),
            )
            .map_err(|error| error.to_string())?;
            let mut buffer = [0u8; 8192];
            while std::io::Read::read(&mut reader, &mut buffer)
                .map_err(|error| error.to_string())?
                != 0
            {}
            if reader.finish().map_err(|error| error.to_string())? != preview.fingerprint {
                return Err("Выбранный файл не соответствует исходному архиву задачи.".into());
            }
            let current = sources.with_ingestion_database(operation, |db| {
                let current = super::job_workflow::read_job(db, &request.job_id)?;
                if current != job {
                    return Err("Состояние импорта изменилось. Повторите выбор источника.".into());
                }
                validate(db, &current, now_ms())?;
                restore_origin(&current, &preview, now_ms())?;
                Ok(current)
            })?;
            Ok((
                saved_origin.unwrap_or_else(|| current_origin.into()),
                current,
            ))
        },
    )
}
fn validate(
    db: &crate::corpus_database::CorpusDatabase,
    job: &StoredJob,
    now: i64,
) -> Result<(), String> {
    use crate::corpus_database::JobStatus;
    if !matches!(job.status, JobStatus::Queued | JobStatus::Paused)
        && !(job.status == JobStatus::Failed
            && job
                .state
                .error
                .as_ref()
                .is_some_and(|error| error.retryable))
    {
        return Err("Задача не ожидает повторного выбора источника.".into());
    }
    super::job_workflow::validate_selection(db, &job.state.selection, now)
}

fn restore_origin(
    job: &StoredJob,
    preview: &super::preview_store::StoredPreview,
    now: i64,
) -> Result<Option<String>, String> {
    match super::adapter::Adapter::resolve(preview)? {
        super::adapter::Adapter::Telegram => {
            super::telegram_worker::restore_origin(job, &preview.fingerprint, now)
        }
        super::adapter::Adapter::SavedPage => {
            super::page_worker::restore_origin(job, preview, now).map(Some)
        }
        super::adapter::Adapter::SelectedUrl => {
            super::page_worker::validate_restore(job, preview, now)?;
            Ok(None)
        }
    }
}

fn restore_url(
    sources: &Sources,
    inputs: &SelectedInputs,
    operation: &Operation,
    request: &RestoreRequest,
    job: &StoredJob,
    preview: &super::preview_store::StoredPreview,
    now_ms: impl Fn() -> i64,
) -> Result<StoredJob, String> {
    super::page_worker::validate_restore(job, preview, now_ms())?;
    inputs
        .urls
        .restore_checked_preview(&request.source_handle, &preview.id, |input| {
            operation.check()?;
            if !preview
                .origin
                .as_ref()
                .is_some_and(|origin| super::page_source::same_url_location(origin, &input.origin))
                || input.sha256 != preview.fingerprint.sha256
                || input.original.len() as u64 != preview.fingerprint.byte_length
                || input.content_type != super::adapter::content_type(preview)?
            {
                return Err(
                    "Повторно выбранная страница не соответствует исходному URL задачи.".into(),
                );
            }
            sources.with_ingestion_database(operation, |db| {
                let current = super::job_workflow::read_job(db, &request.job_id)?;
                if &current != job {
                    return Err("Состояние импорта изменилось. Повторите выбор источника.".into());
                }
                validate(db, &current, now_ms())?;
                super::page_worker::validate_restore(&current, preview, now_ms())?;
                Ok(current)
            })
        })
}
