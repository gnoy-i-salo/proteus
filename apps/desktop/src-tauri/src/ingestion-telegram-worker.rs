//! Bounded archive acquisition. Each checkpoint follows durable records; only a
//! complete stream matching the confirmed preview can publish a snapshot.
use super::telegram_identity::ArchiveVersion;
use super::{
    fingerprint::{FingerprintReader, InputFingerprint},
    grants::authorize_preview_binding,
    operations::Operation,
    preview_store::read_preview,
    telegram_archive_json::{read_telegram_json, ScanControl, TelegramArchiveLimits},
    telegram_document::{normalize_with_version, TelegramDocument},
};
use crate::{
    corpus_database::{
        CorpusDatabase, JobBinding, JobProvider, JobStatus, RevisionInput, StoredJob,
    },
    sources::Sources,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Read, time::Duration};
#[path = "ingestion-telegram-batch.rs"]
mod batch;
use batch::pending;

const INVALID: &str = "Контрольная точка не соответствует выбранному архиву.";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cursor {
    schema_version: u8,
    origin: String,
    fingerprint: InputFingerprint,
    started_at_ms: i64,
    next_message: u64,
    prefix_sha256: String,
    attached_authorities: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Location {
    resource_id: String,
    original_id: String,
    byte_start: u64,
    byte_end: u64,
}
struct Pending {
    revision: RevisionInput,
    location: Location,
    raw_bytes: i64,
}
struct Session<'a, F> {
    sources: &'a Sources,
    operation: &'a Operation,
    job: StoredJob,
    cursor: Cursor,
    version: ArchiveVersion,
    pending: Vec<Pending>,
    now_ms: F,
}
fn acquire<R: Read>(
    sources: &Sources,
    operation: &Operation,
    job_id: &str,
    reader: R,
    origin: &str,
    now_ms: impl Fn() -> i64,
) -> Result<StoredJob, String> {
    super::input::validate_origin(origin)?;
    let (mut job, preview, version) = sources.with_ingestion_database(operation, |db| {
        let job = db.job(job_id)?.ok_or("Задача импорта не найдена.")?;
        authorize(db, &job, now_ms())?;
        let preview = read_preview(db, &job.state.selection.preview_id)?;
        let version = ArchiveVersion::from_adapter(&job.state.checkpoint.adapter_version)?;
        if preview.provider != "telegram" || preview.adapter_version != version.adapter() {
            return Err("Неподдерживаемый адаптер архива.".into());
        }
        Ok((job, preview, version))
    })?;
    if job.status == JobStatus::Completed {
        return publish(sources, operation, job, now_ms());
    }
    let cursor = match &job.state.checkpoint.cursor {
        Some(value) => serde_json::from_str::<Cursor>(value).map_err(|_| INVALID)?,
        None => Cursor {
            schema_version: 1,
            origin: origin.into(),
            fingerprint: preview.fingerprint.clone(),
            started_at_ms: now_ms(),
            next_message: 0,
            prefix_sha256: format!("{:x}", Sha256::digest([])),
            attached_authorities: 0,
        },
    };
    validate_cursor(&job, &cursor, &preview.fingerprint, now_ms())?;
    if cursor.origin != origin {
        return Err(INVALID.into());
    }
    let remaining = remaining_ms(&job, &cursor, now_ms())?;
    // A process can stop after the verified final checkpoint but before the
    // publication transaction. Reuse that durable proof without restaging it.
    if job.state.coverage.complete {
        return publish(sources, operation, job, now_ms());
    }
    job = sources.with_ingestion_database(operation, |db| match job.status {
        JobStatus::Queued => db.start_job(job_id),
        JobStatus::Paused => db.resume_job(job_id),
        JobStatus::Running => Ok(job),
        _ => Err("Задача не готова к чтению архива.".into()),
    })?;
    let limits = TelegramArchiveLimits {
        max_input_bytes: job.state.budget.max_input_bytes as u64,
        max_message_bytes: job.state.budget.max_batch_bytes.min(4 * 1024 * 1024) as usize,
        max_metadata_bytes: job.state.budget.max_batch_bytes.min(65536) as usize,
        max_messages: job.state.budget.max_documents as u64,
        max_duration: Duration::from_millis(remaining as u64),
    };
    let mut session = Session {
        sources,
        operation,
        job,
        cursor,
        version,
        pending: Vec::new(),
        now_ms,
    };
    let mut authorities = vec![session.job.state.selection.preview_id.clone()];
    for binding in &session.job.state.selection.bindings {
        if !authorities.contains(&binding.grant_id) {
            authorities.push(binding.grant_id.clone());
        }
    }
    if session.cursor.attached_authorities > authorities.len() {
        return Err(INVALID.into());
    }
    while session.cursor.attached_authorities < authorities.len() {
        let id = authorities[session.cursor.attached_authorities].clone();
        session.cursor.attached_authorities += 1;
        session.commit(vec![id], 0, 0, false)?;
    }
    let skip = session.cursor.next_message;
    let mut ordinal = 0;
    let mut prefix = format!("{:x}", Sha256::digest([]));
    let mut reader = FingerprintReader::new(
        reader,
        limits.max_input_bytes,
        limits.max_duration,
        operation.cancelled(),
    )
    .map_err(|error| error.to_string())?;
    let summary = read_telegram_json(
        &mut reader,
        &limits,
        operation.cancelled(),
        |_| Ok(()),
        |frame| {
            ordinal += 1;
            let next_prefix = prefix_hash(&prefix, &frame)?;
            if ordinal <= skip {
                prefix = next_prefix;
                if ordinal == skip && prefix != session.cursor.prefix_sha256 {
                    return Err("Сохранённая часть импорта не соответствует выбранному архиву. Начните новую загрузку.".into());
                }
                return Ok(ScanControl::Continue);
            }
            let location = Location {
                resource_id: String::new(),
                original_id: String::new(),
                byte_start: frame.byte_start,
                byte_end: frame.byte_end,
            };
            let document = normalize_with_version(version, origin, frame)?;
            if selected(&session.job.state.selection.bindings, &document)? {
                let item = pending(document, location, version)?;
                if !session.pending.is_empty()
                    && session.batch_size(Some(&item))? > session.job.state.budget.max_batch_bytes
                {
                    session.flush(false)?;
                }
                if session.batch_size(Some(&item))? > session.job.state.budget.max_batch_bytes {
                    return Err("Один документ превышает бюджет сохраняемой порции.".into());
                }
                session.pending.push(item);
            }
            session.cursor.next_message = ordinal;
            prefix = next_prefix;
            session.cursor.prefix_sha256 = prefix.clone();
            if session.pending.len() as i64 >= session.job.state.budget.max_batch_documents {
                session.flush(false)?;
            }
            Ok(ScanControl::Continue)
        },
    )?;
    let fingerprint = reader.finish().map_err(|error| error.to_string())?;
    if !summary.complete || ordinal < skip || fingerprint != session.cursor.fingerprint {
        return Err(
            "Исходный архив изменился или прочитан не полностью. Выберите его заново.".into(),
        );
    }
    session.flush(true)?;
    publish(sources, operation, session.job, (session.now_ms)())
}
fn authorize(db: &CorpusDatabase, job: &StoredJob, now: i64) -> Result<(), String> {
    for binding in &job.state.selection.bindings {
        if binding.provider != JobProvider::Telegram {
            return Err("Источник принадлежит другому сервису.".into());
        }
        authorize_preview_binding(db, binding, &job.state.selection.preview_id, now)?;
    }
    Ok(())
}
fn publish(
    sources: &Sources,
    operation: &Operation,
    job: StoredJob,
    now: i64,
) -> Result<StoredJob, String> {
    sources.with_ingestion_database(operation, |db| {
        authorize(db, &job, now)?;
        if job.state.published_snapshot_id.is_none() {
            let cursor: Cursor =
                serde_json::from_str(job.state.checkpoint.cursor.as_deref().ok_or(INVALID)?)
                    .map_err(|_| INVALID)?;
            remaining_ms(&job, &cursor, now)?;
        }
        if job.status != JobStatus::Completed {
            db.complete_job(&job.id)?;
        }
        operation.check()?;
        super::publication::commit(db, &job.id)?;
        db.job(&job.id)?.ok_or_else(|| INVALID.into())
    })
}
pub(super) fn selected(bindings: &[JobBinding], doc: &TelegramDocument) -> Result<bool, String> {
    for binding in bindings
        .iter()
        .filter(|binding| binding.resource_id == doc.resource_id)
    {
        let author = match &doc.author_id {
            Some(id) => {
                binding.target_author_id.as_ref() == Some(id)
                    || binding.allowed_context_author_ids.contains(id)
            }
            None => binding.allow_unknown_context,
        };
        if !author {
            continue;
        }
        let Some(created) = doc.created_at.as_deref() else {
            if binding.period.include_undated {
                return Ok(true);
            } else {
                continue;
            }
        };
        let date = date_ms(created)?;
        let from = binding.period.from.as_deref().map(date_ms).transpose()?;
        let to = binding.period.to.as_deref().map(date_ms).transpose()?;
        if from.is_none_or(|from| date >= from) && to.is_none_or(|to| date <= to) {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(super) fn date_ms(value: &str) -> Result<i64, String> {
    if let Ok(date) = chrono::DateTime::parse_from_rfc3339(value) {
        return Ok(date.timestamp_millis());
    }
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date| date.and_utc().timestamp_millis())
        .ok_or_else(|| "Неверное время источника.".into())
}
pub(super) fn restore_origin(
    job: &StoredJob,
    fingerprint: &InputFingerprint,
    now: i64,
) -> Result<Option<String>, String> {
    job.state
        .checkpoint
        .cursor
        .as_deref()
        .map(|value| {
            let cursor: Cursor = serde_json::from_str(value).map_err(|_| INVALID)?;
            validate_cursor(job, &cursor, fingerprint, now)?;
            Ok(cursor.origin)
        })
        .transpose()
}
pub(crate) fn completed_origin(
    job: &StoredJob,
    fingerprint: &InputFingerprint,
    now: i64,
) -> Result<String, String> {
    if job.status != JobStatus::Completed
        || !job.state.coverage.complete
        || job.state.published_snapshot_id.is_none()
    {
        return Err(INVALID.into());
    }
    let cursor: Cursor =
        serde_json::from_str(job.state.checkpoint.cursor.as_deref().ok_or(INVALID)?)
            .map_err(|_| INVALID)?;
    validate_cursor_data(job, &cursor, fingerprint, now)?;
    let authorities = job
        .state
        .selection
        .bindings
        .iter()
        .map(|binding| binding.grant_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if cursor.attached_authorities != authorities.len() + 1 || job.state.error.is_some() {
        return Err(INVALID.into());
    }
    Ok(cursor.origin)
}
fn validate_cursor(
    job: &StoredJob,
    cursor: &Cursor,
    fingerprint: &InputFingerprint,
    now: i64,
) -> Result<(), String> {
    validate_cursor_data(job, cursor, fingerprint, now)?;
    remaining_ms(job, cursor, now)?;
    Ok(())
}
fn validate_cursor_data(
    job: &StoredJob,
    cursor: &Cursor,
    fingerprint: &InputFingerprint,
    now: i64,
) -> Result<(), String> {
    if cursor.schema_version != 1
        || &cursor.fingerprint != fingerprint
        || cursor.started_at_ms < 0
        || cursor.started_at_ms > now
        || cursor.next_message > job.state.budget.max_documents as u64
        || cursor.prefix_sha256.len() != 64
        || !cursor
            .prefix_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || (cursor.next_message == 0 && cursor.prefix_sha256 != format!("{:x}", Sha256::digest([])))
    {
        return Err(INVALID.into());
    }
    super::input::validate_origin(&cursor.origin)?;
    Ok(())
}
fn remaining_ms(job: &StoredJob, cursor: &Cursor, now: i64) -> Result<i64, String> {
    let elapsed = now
        .checked_sub(cursor.started_at_ms)
        .filter(|elapsed| *elapsed >= 0)
        .ok_or(INVALID)?;
    job.state
        .budget
        .max_duration_ms
        .checked_sub(elapsed)
        .filter(|left| *left > 0)
        .ok_or_else(|| "Превышено общее время импорта.".into())
}

// Hash precisely the frames staged before a checkpoint, independently of read
// buffering. A retry must not mix an earlier changed prefix with a valid tail.
fn prefix_hash(
    previous: &str,
    frame: &super::telegram_archive_json::TelegramMessageFrame,
) -> Result<String, String> {
    let header = serde_json::to_vec(&(
        &frame.chat.id,
        &frame.chat.name,
        &frame.chat.kind,
        frame.byte_start,
        frame.byte_end,
    ))
    .map_err(|_| INVALID)?;
    let mut hash = Sha256::new();
    hash.update(b"telegramPrefix:1\0");
    hash.update(previous.as_bytes());
    hash.update((header.len() as u64).to_be_bytes());
    hash.update(header);
    hash.update(&frame.raw_json);
    Ok(format!("{:x}", hash.finalize()))
}

#[derive(Debug)]
pub struct Failure {
    pub message: String,
    pub retryable: bool,
}
pub fn run_reader<R: Read>(
    sources: &Sources,
    operation: &Operation,
    job_id: &str,
    reader: R,
    origin: &str,
    now_ms: impl Fn() -> i64,
) -> Result<StoredJob, String> {
    run_reader_outcome(sources, operation, job_id, reader, origin, now_ms)
        .map_err(|failure| failure.message)
}
pub fn run_reader_outcome<R: Read>(
    sources: &Sources,
    operation: &Operation,
    job_id: &str,
    reader: R,
    origin: &str,
    now_ms: impl Fn() -> i64,
) -> Result<StoredJob, Failure> {
    let read_failed = std::cell::Cell::new(false);
    let reader = TrackedReader {
        reader,
        failed: &read_failed,
    };
    acquire(sources, operation, job_id, reader, origin, now_ms).map_err(|message| Failure {
        retryable: read_failed.get() || message == crate::corpus_database::STORAGE_ERROR,
        message,
    })
}
// Record failures of the actual input reader separately from parser, fingerprint,
// cancellation and budget checks. Their user-facing text is not an error codec.
struct TrackedReader<'a, R> {
    reader: R,
    failed: &'a std::cell::Cell<bool>,
}
impl<R: Read> Read for TrackedReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.reader
            .read(buffer)
            .inspect_err(|_| self.failed.set(true))
    }
}
