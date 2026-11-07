use super::job_budget::{encode_state, MAX_STATE_BYTES};
use super::job_types::*;
use super::types::RecordGeneration;
use super::{sql_error, INVALID};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::io::Write;
pub(super) const NOT_FOUND: &str = "Задача импорта не найдена.";
struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(super) fn job_hash(id: &str, status: JobStatus, phase: JobPhase, state: &JobState) -> String {
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(&mut writer, &(id, status, phase, state))
        .expect("serializing typed job state cannot fail");
    format!("{:x}", writer.0.finalize())
}
pub(super) fn list_jobs(
    connection: &Connection,
    status: Option<JobStatus>,
) -> Result<Vec<StoredJob>, String> {
    let (sql, value): (&str, Option<String>) = match status {
        Some(status) => (
            "SELECT id FROM jobs WHERE status=?1 ORDER BY rowid",
            Some(status.as_str().to_string()),
        ),
        None => ("SELECT id FROM jobs ORDER BY rowid", None),
    };
    let mut query = connection.prepare(sql).map_err(sql_error)?;
    let ids = match &value {
        Some(status) => query
            .query_map([status.as_str()], |row| row.get(0))
            .map_err(sql_error)?
            .collect::<Result<Vec<String>, _>>()
            .map_err(sql_error)?,
        None => query
            .query_map([], |row| row.get(0))
            .map_err(sql_error)?
            .collect::<Result<Vec<String>, _>>()
            .map_err(sql_error)?,
    };
    let mut jobs = Vec::new();
    for id in ids {
        if let Some(job) = read_job(connection, &id)? {
            jobs.push(job);
        }
    }
    Ok(jobs)
}
pub(super) fn running_job_ids(connection: &Connection) -> Result<Vec<String>, String> {
    let mut query = connection
        .prepare("SELECT id FROM jobs WHERE status='running' ORDER BY rowid")
        .map_err(sql_error)?;
    let ids = query
        .query_map([], |row| row.get(0))
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    Ok(ids)
}
pub(super) fn job_record_ids_in(
    connection: &Connection,
    job_id: &str,
) -> Result<Vec<String>, String> {
    let mut query = connection
        .prepare("SELECT record_id FROM job_records WHERE job_id=?1 ORDER BY position")
        .map_err(sql_error)?;
    let ids = query
        .query_map([job_id], |row| row.get(0))
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    Ok(ids)
}
pub(super) fn job_record_references(
    connection: &Connection,
    job_id: &str,
) -> Result<Vec<RecordGeneration>, String> {
    let mut query = connection
        .prepare(
            "SELECT record_id,record_generation FROM job_records WHERE job_id=?1 ORDER BY position",
        )
        .map_err(sql_error)?;
    let references = query
        .query_map([job_id], |row| {
            Ok(RecordGeneration {
                id: row.get(0)?,
                generation: row.get(1)?,
            })
        })
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    Ok(references)
}
pub(super) fn read_job(connection: &Connection, id: &str) -> Result<Option<StoredJob>, String> {
    let row: Option<(i64, String, String, String, String)> = connection
        .query_row(
            "SELECT schema_version,status,phase,state_json,canonical_sha256,length(CAST(state_json AS BLOB)) FROM jobs WHERE id=?1",
            [id],
            |row| {
                if row.get::<_,i64>(5)? > MAX_STATE_BYTES as i64 { return Err(rusqlite::Error::InvalidQuery); }
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(sql_error)?;
    let Some((schema_version, status, phase, state_json, hash)) = row else {
        return Ok(None);
    };
    if schema_version != 1 {
        return Err(INVALID.into());
    }
    let status: JobStatus = serde_json::from_value(serde_json::Value::String(status))
        .map_err(|_| INVALID.to_string())?;
    let phase: JobPhase = serde_json::from_value(serde_json::Value::String(phase))
        .map_err(|_| INVALID.to_string())?;
    let state: JobState = serde_json::from_str(&state_json).map_err(|_| INVALID.to_string())?;
    if job_hash(id, status, phase, &state) != hash {
        return Err(INVALID.into());
    }
    Ok(Some(StoredJob {
        id: id.into(),
        status,
        phase,
        state,
        hash,
    }))
}
pub(super) fn write_job(connection: &Connection, job: &StoredJob) -> Result<(), String> {
    let state_json = encode_state(&job.state)?;
    let updated = connection
        .execute(
            "UPDATE jobs SET status=?2,phase=?3,state_json=?4,canonical_sha256=?5 WHERE id=?1",
            params![
                job.id,
                job.status.as_str(),
                job.phase.as_str(),
                state_json,
                job.hash
            ],
        )
        .map_err(sql_error)?;
    if updated != 1 {
        return Err(INVALID.into());
    }
    Ok(())
}
pub(super) fn transition_job(
    connection: &Connection,
    id: &str,
    allowed: &[JobStatus],
    to: JobStatus,
    mutate: impl FnOnce(&mut StoredJob),
) -> Result<StoredJob, String> {
    let mut job = read_job(connection, id)?.ok_or(NOT_FOUND)?;
    if !allowed.contains(&job.status) {
        return Err("Недопустимый переход состояния задачи импорта.".into());
    }
    mutate(&mut job);
    job.status = to;
    job.hash = job_hash(&job.id, job.status, job.phase, &job.state);
    write_job(connection, &job)?;
    let restored = read_job(connection, id)?.ok_or(INVALID)?;
    if restored != job {
        return Err(INVALID.into());
    }
    Ok(restored)
}
