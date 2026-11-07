//! Ingestion revocation shares the canonical database and preserves other owners.
use super::job_store::{job_hash, read_job, write_job};
use super::record_store::remove_record;
use super::{sql_error, CorpusDatabase, JobError, JobProvider, JobStatus, INVALID};
use rusqlite::{params, TransactionBehavior};
impl CorpusDatabase {
    pub fn revoke_ingestion_provider(&mut self, provider: JobProvider) -> Result<(), String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        // Retain at most one bounded job payload and one page of IDs at a time.
        let mut after = String::new();
        loop {
            let ids = {
                let mut query = tx
                    .prepare("SELECT id FROM jobs WHERE id>?1 ORDER BY id LIMIT 32")
                    .map_err(sql_error)?;
                let rows = query
                    .query_map([&after], |row| row.get::<_, String>(0))
                    .map_err(sql_error)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)?
            };
            if ids.is_empty() {
                break;
            }
            for id in &ids {
                let mut job = read_job(&tx, id)?.ok_or(INVALID)?;
                if matches!(job.status, JobStatus::Completed | JobStatus::Cancelled)
                    || !job
                        .state
                        .selection
                        .bindings
                        .iter()
                        .any(|binding| binding.provider == provider)
                {
                    continue;
                }
                job.status = JobStatus::Cancelled;
                job.state.error = Some(JobError {
                    code: "source-revoked".into(),
                    message: "Данные сервиса удалены; выберите источник заново.".into(),
                    retryable: false,
                });
                job.hash = job_hash(&job.id, job.status, job.phase, &job.state);
                write_job(&tx, &job)?;
            }
            after = ids.last().ok_or(INVALID)?.clone();
        }
        let mut after = String::new();
        loop {
            let ids = {
                let mut query = tx.prepare("SELECT id FROM records WHERE namespace='ingestion' AND provider=?1 AND kind<>'deleted' AND id>?2 ORDER BY id LIMIT 32").map_err(sql_error)?;
                let rows = query
                    .query_map(params![provider.key(), after], |row| {
                        row.get::<_, String>(0)
                    })
                    .map_err(sql_error)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)?
            };
            if ids.is_empty() {
                break;
            }
            for id in &ids {
                remove_record(&tx, id)?;
            }
            after = ids.last().ok_or(INVALID)?.clone();
        }
        tx.commit().map_err(sql_error)
    }
}
