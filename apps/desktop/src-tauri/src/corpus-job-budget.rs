//! Storage limits are measured before materializing record payloads. Input-file
//! bytes and wall-clock duration remain acquisition-worker responsibilities.
use super::job_types::{JobSelection, JobState, StoredJob};
use super::{sql_error, INVALID};
use rusqlite::{Connection, OptionalExtension};
use std::io::{Error, Write};
pub(super) const MAX_STATE_BYTES: usize = 32 * 1024 * 1024;

struct SizeCounter(usize);
impl Write for SizeCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|size| *size <= MAX_STATE_BYTES)
            .ok_or_else(|| Error::other("job state size limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn validate_size(value: &impl serde::Serialize) -> Result<(), String> {
    serde_json::to_writer(SizeCounter(0), value)
        .map_err(|_| "Превышен допустимый размер состояния задачи импорта.".into())
}
pub(super) fn validate_selection_size(selection: &JobSelection) -> Result<(), String> {
    validate_size(selection)
}
pub(super) fn encode_state(state: &JobState) -> Result<String, String> {
    validate_size(state)?;
    serde_json::to_string(state).map_err(|_| INVALID.into())
}

// Include opaque record metadata as well as document/provenance bodies. BLOB
// length measures UTF-8 bytes; SQLite TEXT length would count Unicode characters.
const RECORD_BYTES: &str = r#"
    length(CAST(records.state_json AS BLOB))
    + length(CAST(records.id AS BLOB))
    + length(CAST(records.provider AS BLOB))
    + COALESCE((
        SELECT sum(
            length(CAST(revisions.document_json AS BLOB))
            + length(CAST(revisions.provenance_json AS BLOB))
            + length(CAST(revisions.source_key AS BLOB))
            + length(CAST(revisions.document_id AS BLOB))
        )
        FROM record_revisions JOIN revisions ON revisions.id=record_revisions.revision_id
        WHERE record_revisions.record_id=records.id
    ),0)
"#;

pub(super) fn validate_payloads(
    connection: &Connection,
    job: &StoredJob,
    ids: &[String],
) -> Result<(), String> {
    let stale: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM job_records JOIN records ON records.id=job_records.record_id
         WHERE job_records.job_id=?1 AND (records.kind='deleted' OR records.generation<>job_records.record_generation))",
        [&job.id], |row| row.get(0),
    ).map_err(sql_error)?;
    if stale {
        return Err("Поколение источника изменилось после фиксации порции.".into());
    }
    // Only constant SQL is composed here; every identifier from callers is bound.
    let single_query =
        format!("SELECT records.kind,{RECORD_BYTES} FROM records WHERE records.id=?1");
    let mut batch_bytes = 0i64;
    for id in ids {
        let measured: Option<(String, i64)> = connection
            .query_row(&single_query, [id], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()
            .map_err(sql_error)?;
        let (kind, bytes) = measured.ok_or("Запись задачи отсутствует или отозвана.")?;
        if kind == "deleted" {
            return Err("Запись задачи отсутствует или отозвана.".into());
        }
        batch_bytes = batch_bytes.checked_add(bytes).ok_or(INVALID)?;
        if batch_bytes > job.state.budget.max_batch_bytes {
            return Err("Превышен лимит сохранённых данных порции.".into());
        }
    }
    let total_query = format!("SELECT COALESCE(sum({RECORD_BYTES}),0)
        FROM job_records JOIN records ON records.id=job_records.record_id WHERE job_records.job_id=?1");
    let retained: i64 = connection
        .query_row(&total_query, [&job.id], |row| row.get(0))
        .map_err(sql_error)?;
    if retained.checked_add(batch_bytes).ok_or(INVALID)? > job.state.budget.max_expanded_bytes {
        return Err("Превышен лимит сохранённых данных импорта.".into());
    }
    Ok(())
}
