use super::{operations::Operation, SelectedInputs};
use crate::{
    corpus_database::{JobBudget, JobPhase, JobSelection, JobState, JobStatus, StoredJob},
    sources::Sources,
};
use serde::{Deserialize, Serialize};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartRequest {
    pub operation_id: String,
    pub selection: JobSelection,
    pub budget: JobBudget,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    schema_version: u8,
    id: String,
    status: JobStatus,
    phase: JobPhase,
    #[serde(flatten)]
    state: JobState,
}
impl From<StoredJob> for JobView {
    fn from(job: StoredJob) -> Self {
        Self {
            schema_version: 1,
            id: job.id,
            status: job.status,
            phase: job.phase,
            state: job.state,
        }
    }
}
#[derive(Clone, Copy)]
pub enum Control {
    Pause,
    Cancel,
}
#[derive(Clone, Copy)]
pub enum Restart {
    Resume,
    Retry,
}
pub fn create(
    sources: &Sources,
    inputs: &SelectedInputs,
    operation: &Operation,
    request: StartRequest,
    now_ms: i64,
) -> Result<StoredJob, String> {
    if request.operation_id != operation.id() {
        return Err("Идентификатор задачи не соответствует операции.".into());
    }
    operation.check()?;
    inputs.read_preview(&request.selection.preview_id, |_, _| Ok(()))?;
    sources.with_ingestion_database(operation, |db| {
        validate_selection(db, &request.selection, now_ms)?;
        let preview = super::preview_store::read_preview(db, &request.selection.preview_id)?;
        let job = db.create_job(&crate::corpus_database::JobInput {
            id: request.operation_id,
            adapter_version: preview.adapter_version,
            selection: request.selection,
            budget: request.budget,
        })?;
        if job.status == JobStatus::Queued {
            db.start_job(&job.id)
        } else {
            Ok(job)
        }
    })
}
pub fn inspect(sources: &Sources, job_id: &str) -> Result<StoredJob, String> {
    sources.with_ingestion_job_state(|db| read_job(db, job_id))
}
pub fn control(sources: &Sources, job_id: &str, action: Control) -> Result<StoredJob, String> {
    sources.with_ingestion_job_state(|db| {
        let job = read_job(db, job_id)?;
        let result = match action {
            Control::Pause if job.status == JobStatus::Paused => job,
            Control::Cancel if job.status == JobStatus::Cancelled => job,
            Control::Pause => {
                if job.status == JobStatus::Queued {
                    db.start_job(job_id)?;
                }
                db.pause_job(job_id)?
            }
            Control::Cancel => db.cancel_job(job_id)?,
        };
        // Change durable state and cancel the old reader while holding the gate;
        // a racing worker cannot resume or commit between these two actions.
        sources.cancel_ingestion_operation(job_id)?;
        Ok(result)
    })
}
pub fn restart(
    sources: &Sources,
    inputs: &SelectedInputs,
    job_id: &str,
    action: Restart,
    now_ms: i64,
) -> Result<(StoredJob, Operation), String> {
    let operation =
        sources.begin_ingestion_operation(job_id, crate::corpus_database::JobProvider::Telegram)?;
    let prior = inspect(sources, job_id)?;
    inputs.read_preview(&prior.state.selection.preview_id, |_, _| Ok(()))?;
    let job = sources.with_ingestion_database(&operation, |db| {
        let job = read_job(db, job_id)?;
        validate_selection(db, &job.state.selection, now_ms)?;
        match action {
            Restart::Resume => db.resume_job(job_id),
            Restart::Retry => {
                db.retry_job(job_id)?;
                db.start_job(job_id)
            }
        }
    })?;
    Ok((job, operation))
}
pub(super) fn read_job(
    db: &crate::corpus_database::CorpusDatabase,
    job_id: &str,
) -> Result<StoredJob, String> {
    if job_id.is_empty() || job_id.len() > 128 {
        return Err("Неверный идентификатор задачи.".into());
    }
    db.job(job_id)?
        .ok_or_else(|| "Задача импорта не найдена.".into())
}
pub(super) fn validate_selection(
    db: &crate::corpus_database::CorpusDatabase,
    selection: &JobSelection,
    now_ms: i64,
) -> Result<(), String> {
    let preview = super::preview_store::read_preview(db, &selection.preview_id)?;
    if preview.provider != "telegram" {
        return Err("Этот формат ещё не поддерживает потоковый импорт.".into());
    }
    super::telegram_identity::ArchiveVersion::from_adapter(&preview.adapter_version)?;
    for binding in &selection.bindings {
        super::grants::authorize_preview_binding(db, binding, &selection.preview_id, now_ms)?;
    }
    Ok(())
}

pub fn execute(
    sources: &Sources,
    inputs: &SelectedInputs,
    operation: &Operation,
    now_ms: impl Fn() -> i64,
) -> Result<StoredJob, String> {
    let attempt = (|| {
        let job = sources
            .with_ingestion_database(operation, |db| read_job(db, operation.id()))
            .map_err(|message| super::telegram_worker::Failure {
                message,
                retryable: false,
            })?;
        inputs
            .read_preview(&job.state.selection.preview_id, |file, origin| {
                Ok(super::telegram_worker::run_reader_outcome(
                    sources, operation, &job.id, file, origin, &now_ms,
                ))
            })
            .map_err(|message| super::telegram_worker::Failure {
                message,
                retryable: true,
            })?
    })();
    match attempt {
        Ok(job) => Ok(job),
        Err(failure) => finish_failure(sources, operation, failure, now_ms()),
    }
}
pub(crate) fn finish_failure(
    sources: &Sources,
    operation: &Operation,
    failure: super::telegram_worker::Failure,
    now_ms: i64,
) -> Result<StoredJob, String> {
    sources.with_ingestion_job_state(|db| {
        let job = read_job(db, operation.id())?;
        if job.status != JobStatus::Running || !operation.is_current()? {
            return Ok(job);
        }
        if operation
            .cancelled()
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return if operation.shutdown_requested()? {
                db.pause_job(&job.id)
            } else {
                db.cancel_job(&job.id)
            };
        }
        let authority = validate_selection(db, &job.state.selection, now_ms);
        let retryable = failure.retryable && authority.is_ok();
        let mut message = authority.err().unwrap_or(failure.message);
        if message.len() > 2048 {
            let mut end = 2048;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
        }
        db.fail_job(
            &job.id,
            &crate::corpus_database::JobError {
                code: "acquisition-failed".into(),
                message,
                retryable,
            },
        )
    })
}
