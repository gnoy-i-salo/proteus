//! One bounded saved page. Only a fully verified original can enter a published
//! snapshot; authority checkpoints remain resumable without retaining source paths.
use super::{
    adapter::{content_type, Adapter},
    fingerprint::InputFingerprint,
    operations::Operation,
    page_document::{PageDocument, PageOrigin},
    preview_store::{read_preview, StoredPreview},
    worker_outcome::{Failure, TrackedReader},
};
use crate::{
    corpus_database::{
        Acquisition, HashAlgorithm, JobBatch, JobCheckpoint, JobPhase, JobProvider, JobStatus,
        ProvenanceLimitation, RecordInput, RevisionInput, SourceProvenance, StoredJob,
    },
    sources::Sources,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;

const INVALID: &str = "Контрольная точка не соответствует выбранной странице.";
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cursor {
    schema_version: u8,
    preview_id: String,
    fingerprint: InputFingerprint,
    started_at_ms: i64,
    attached_authorities: usize,
}
struct Session<'a, F> {
    sources: &'a Sources,
    operation: &'a Operation,
    job: StoredJob,
    cursor: Cursor,
    now_ms: F,
}
pub fn run_reader_outcome<R: Read>(
    sources: &Sources,
    operation: &Operation,
    job_id: &str,
    reader: R,
    origin: &str,
    now_ms: impl Fn() -> i64,
) -> Result<StoredJob, Failure> {
    let failed = std::cell::Cell::new(false);
    acquire(
        sources,
        operation,
        job_id,
        TrackedReader {
            reader,
            failed: &failed,
        },
        origin,
        now_ms,
    )
    .map_err(|message| Failure {
        retryable: failed.get() || message == crate::corpus_database::STORAGE_ERROR,
        message,
    })
}
fn acquire<R: Read>(
    sources: &Sources,
    operation: &Operation,
    job_id: &str,
    reader: R,
    origin: &str,
    now_ms: impl Fn() -> i64,
) -> Result<StoredJob, String> {
    let (job, preview) = sources.with_ingestion_database(operation, |db| {
        let job = super::job_workflow::read_job(db, job_id)?;
        super::job_workflow::validate_selection(db, &job.state.selection, now_ms())?;
        let preview = read_preview(db, &job.state.selection.preview_id)?;
        if Adapter::resolve(&preview)?.provider() != JobProvider::SavedPage
            || operation.provider() != JobProvider::SavedPage
            || job.state.checkpoint.adapter_version != preview.adapter_version
        {
            return Err(INVALID.into());
        }
        Ok((job, preview))
    })?;
    let Some(PageOrigin::SavedPage {
        source_origin,
        label,
    }) = &preview.origin
    else {
        return Err(INVALID.into());
    };
    if origin != source_origin {
        return Err(INVALID.into());
    }
    let cursor = match job.state.checkpoint.cursor.as_deref() {
        Some(value) => serde_json::from_str::<Cursor>(value).map_err(|_| INVALID)?,
        None if job.state.checkpoint.committed_batch == 0 => Cursor {
            schema_version: 1,
            preview_id: preview.id.clone(),
            fingerprint: preview.fingerprint.clone(),
            started_at_ms: now_ms(),
            attached_authorities: 0,
        },
        None => return Err(INVALID.into()),
    };
    validate_cursor(&job, &preview, &cursor, now_ms())?;
    let mut session = Session {
        sources,
        operation,
        job,
        cursor,
        now_ms,
    };
    if session.job.state.coverage.complete {
        return session.publish();
    }
    if session.job.status != JobStatus::Running {
        return Err("Задача не готова к чтению страницы.".into());
    }
    session.check()?;
    let mut authorities = vec![preview.id.clone()];
    for binding in &session.job.state.selection.bindings {
        if !authorities.contains(&binding.grant_id) {
            authorities.push(binding.grant_id.clone());
        }
    }
    while session.cursor.attached_authorities < authorities.len() {
        let id = authorities[session.cursor.attached_authorities].clone();
        session.cursor.attached_authorities += 1;
        session.commit(vec![id], 0, 0, false)?;
    }
    let document = super::page_preview::read_saved(
        reader,
        source_origin,
        label,
        content_type(&preview)?,
        &session.job.state.budget,
        operation,
        &mut || session.check(),
    )?;
    if document.sha256 != preview.fingerprint.sha256
        || document.original.len() as u64 != preview.fingerprint.byte_length
        || preview.resources.len() != 1
        || preview.resources[0].id != document.source_key
    {
        return Err("Исходная страница изменилась. Выберите её заново.".into());
    }
    let selected = selected(&session.job, &document)?;
    let bytes = if selected {
        document.original.len() as i64
    } else {
        0
    };
    let mut ids = Vec::new();
    if selected {
        let record = record(&session.job, &preview, document)?;
        sources.with_ingestion_database(operation, |db| {
            session.check()?;
            super::job_workflow::validate_selection(
                db,
                &session.job.state.selection,
                (session.now_ms)(),
            )?;
            if super::job_workflow::read_job(db, job_id)? != session.job {
                return Err(INVALID.into());
            }
            db.save_ingestion(&record)?;
            Ok(())
        })?;
        ids.push(record.id);
    }
    session.commit(ids, i64::from(selected), bytes, true)?;
    session.publish()
}
fn validate_cursor(
    job: &StoredJob,
    preview: &StoredPreview,
    cursor: &Cursor,
    now: i64,
) -> Result<(), String> {
    let authorities = job
        .state
        .selection
        .bindings
        .iter()
        .map(|b| &b.grant_id)
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        + 1;
    if cursor.schema_version != 1
        || cursor.preview_id != preview.id
        || cursor.fingerprint != preview.fingerprint
        || cursor.started_at_ms < 0
        || cursor.started_at_ms > now
        || cursor.attached_authorities > authorities
        || (job.state.coverage.complete
            && (cursor.attached_authorities != authorities
                || job.state.coverage.loaded_documents > 1))
    {
        return Err(INVALID.into());
    }
    Ok(())
}
impl<F: Fn() -> i64> Session<'_, F> {
    fn check(&self) -> Result<(), String> {
        self.operation.check()?;
        let elapsed = (self.now_ms)()
            .checked_sub(self.cursor.started_at_ms)
            .filter(|v| *v >= 0)
            .ok_or(INVALID)?;
        if elapsed >= self.job.state.budget.max_duration_ms {
            return Err("Превышено общее время импорта.".into());
        }
        Ok(())
    }
    fn commit(
        &mut self,
        record_ids: Vec<String>,
        loaded_documents: i64,
        loaded_bytes: i64,
        complete: bool,
    ) -> Result<(), String> {
        self.check()?;
        let batch = JobBatch {
            phase: Some(if complete { JobPhase::Publishing } else { JobPhase::Reading }),
            checkpoint: JobCheckpoint {
                adapter_version: self.job.state.checkpoint.adapter_version.clone(),
                cursor: Some(serde_json::to_string(&self.cursor).map_err(|_| INVALID)?),
                committed_batch: self.job.state.checkpoint.committed_batch + 1,
                staging_generation: self.job.state.checkpoint.staging_generation + 1,
            },
            loaded_documents, loaded_bytes, indexed_documents: 0, analyzed_documents: 0, used_documents: 0,
            total_documents: complete.then_some(loaded_documents), complete, record_ids,
            limitations: vec!["Покрытие отражает только загруженные страницы; индексация и модельное изучение ещё не выполнены. Имя автора в разметке не подтверждает личность; вложения не загружены. Покрытие учитывает выбранный период и разрешённый неизвестный контекст.".into()],
        };
        self.job = self.sources.with_ingestion_database(self.operation, |db| {
            self.check()?;
            super::job_workflow::validate_selection(
                db,
                &self.job.state.selection,
                (self.now_ms)(),
            )?;
            db.commit_job_batch(&self.job.id, &self.job.hash, &batch)
        })?;
        Ok(())
    }
    fn publish(self) -> Result<StoredJob, String> {
        self.sources.with_ingestion_database(self.operation, |db| {
            super::job_workflow::validate_selection(
                db,
                &self.job.state.selection,
                (self.now_ms)(),
            )?;
            if self.job.state.published_snapshot_id.is_none() {
                self.check()?;
            }
            if super::job_workflow::read_job(db, &self.job.id)? != self.job {
                return Err(INVALID.into());
            }
            if self.job.status != JobStatus::Completed {
                db.complete_job(&self.job.id)?;
            }
            self.operation.check()?;
            super::publication::commit(db, &self.job.id)?;
            super::job_workflow::read_job(db, &self.job.id)
        })
    }
}
fn selected(job: &StoredJob, doc: &PageDocument) -> Result<bool, String> {
    for binding in &job.state.selection.bindings {
        if binding.resource_id != doc.source_key || !binding.allow_unknown_context {
            continue;
        }
        let Some(created) = &doc.published_at else {
            if binding.period.include_undated {
                return Ok(true);
            }
            continue;
        };
        let date = super::period::date_ms(created)?;
        let from = binding
            .period
            .from
            .as_deref()
            .map(super::period::date_ms)
            .transpose()?;
        let to = binding
            .period
            .to
            .as_deref()
            .map(super::period::date_ms)
            .transpose()?;
        if from.is_none_or(|from| date >= from) && to.is_none_or(|to| date <= to) {
            return Ok(true);
        }
    }
    Ok(false)
}
fn record(
    job: &StoredJob,
    preview: &StoredPreview,
    document: PageDocument,
) -> Result<RecordInput, String> {
    Ok(RecordInput {
        id: format!("pageImport:{:x}", Sha256::digest(job.id.as_bytes())),
        kind: "import".into(), provider: "savedPage".into(),
        state_json: serde_json::to_string(&serde_json::json!({"schemaVersion":1,"codec":"savedPageBatch:1","previewId":preview.id})).map_err(|_| INVALID)?,
        documents: vec![RevisionInput {
            source_key: document.source_key.clone(), document_id: document.id.clone(),
            document_json: serde_json::to_string(&document).map_err(|_| INVALID)?,
            provenance: SourceProvenance {
                acquisition: Acquisition::SavedPage, adapter_version: preview.adapter_version.clone(),
                source_format: preview.source_format.clone(), original_object_id: None,
                original_byte_length: Some(document.original.len() as u64), hash_algorithm: HashAlgorithm::Sha256,
                limitations: vec![ProvenanceLimitation::OriginalObjectIdUnavailable, ProvenanceLimitation::AttachmentsUnavailable, ProvenanceLimitation::RelationsIncomplete],
            },
        }],
    })
}

pub(super) fn restore_origin(
    job: &StoredJob,
    preview: &StoredPreview,
    now: i64,
) -> Result<String, String> {
    if job.state.checkpoint.adapter_version != preview.adapter_version {
        return Err(INVALID.into());
    }
    if let Some(value) = &job.state.checkpoint.cursor {
        let cursor: Cursor = serde_json::from_str(value).map_err(|_| INVALID)?;
        validate_cursor(job, preview, &cursor, now)?;
        if now - cursor.started_at_ms >= job.state.budget.max_duration_ms {
            return Err("Превышено общее время импорта.".into());
        }
    } else if job.state.checkpoint.committed_batch != 0 {
        return Err(INVALID.into());
    }
    match &preview.origin {
        Some(PageOrigin::SavedPage { source_origin, .. }) => Ok(source_origin.clone()),
        _ => Err(INVALID.into()),
    }
}
