//! Persistent ingestion lifecycle; workers and IPC remain separate owners.
use super::job_batch::apply_batch;
use super::job_budget::encode_state;
use super::job_store::{
    job_hash, job_record_ids_in, job_record_references, list_jobs, read_job, running_job_ids,
    transition_job, write_job, NOT_FOUND,
};
use super::job_types::*;
use super::job_validation::{validate_error, validate_input};
use super::snapshot_store::{publish_snapshot, read_snapshot};
use super::types::Snapshot;
use super::{sql_error, CorpusDatabase, INVALID};
use rusqlite::{params, TransactionBehavior};
const CONFLICT: &str = "Состояние задачи изменилось; повторите чтение и попробуйте снова.";
const INTERRUPTED: &str = "interrupted";
impl CorpusDatabase {
    pub fn create_job(&mut self, input: &JobInput) -> Result<StoredJob, String> {
        validate_input(input)?;
        let state = JobState {
            selection: input.selection.clone(),
            budget: input.budget.clone(),
            checkpoint: JobCheckpoint {
                adapter_version: input.adapter_version.clone(),
                cursor: None,
                committed_batch: 0,
                staging_generation: 0,
            },
            coverage: JobCoverage {
                loaded_documents: 0,
                indexed_documents: 0,
                analyzed_documents: 0,
                used_documents: 0,
                total_documents: None,
                loaded_bytes: 0,
                complete: false,
                limitations: Vec::new(),
            },
            published_snapshot_id: None,
            error: None,
        };
        let state_json = encode_state(&state)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        if let Some(existing) = read_job(&tx, &input.id)? {
            return if existing.state.selection == state.selection
                && existing.state.budget == state.budget
                && existing.state.checkpoint.adapter_version == input.adapter_version
            {
                Ok(existing)
            } else {
                Err("Задача импорта с таким идентификатором уже существует.".into())
            };
        }
        let job = StoredJob {
            hash: job_hash(&input.id, JobStatus::Queued, JobPhase::Reading, &state),
            id: input.id.clone(),
            status: JobStatus::Queued,
            phase: JobPhase::Reading,
            state,
        };
        tx.execute(
            "INSERT INTO jobs(id,schema_version,status,phase,state_json,canonical_sha256) VALUES(?1,1,'queued','reading',?2,?3)",
            params![job.id, state_json, job.hash],
        )
        .map_err(sql_error)?;
        let restored = read_job(&tx, &input.id)?.ok_or(INVALID)?;
        if restored != job {
            return Err(INVALID.into());
        }
        tx.commit().map_err(sql_error)?;
        Ok(restored)
    }
    pub fn job(&self, id: &str) -> Result<Option<StoredJob>, String> {
        read_job(&self.connection, id)
    }
    pub fn jobs(&self) -> Result<Vec<StoredJob>, String> {
        list_jobs(&self.connection, None)
    }
    pub fn jobs_by_status(&self, status: JobStatus) -> Result<Vec<StoredJob>, String> {
        list_jobs(&self.connection, Some(status))
    }
    pub fn start_job(&mut self, id: &str) -> Result<StoredJob, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let job = transition_job(&tx, id, &[JobStatus::Queued], JobStatus::Running, |_| {})?;
        tx.commit().map_err(sql_error)?;
        Ok(job)
    }
    pub fn pause_job(&mut self, id: &str) -> Result<StoredJob, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let job = transition_job(&tx, id, &[JobStatus::Running], JobStatus::Paused, |_| {})?;
        tx.commit().map_err(sql_error)?;
        Ok(job)
    }
    pub fn resume_job(&mut self, id: &str) -> Result<StoredJob, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let job = transition_job(&tx, id, &[JobStatus::Paused], JobStatus::Running, |job| {
            job.state.error = None;
        })?;
        tx.commit().map_err(sql_error)?;
        Ok(job)
    }
    pub fn cancel_job(&mut self, id: &str) -> Result<StoredJob, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let job = transition_job(
            &tx,
            id,
            &[JobStatus::Queued, JobStatus::Running, JobStatus::Paused],
            JobStatus::Cancelled,
            |_| {},
        )?;
        tx.commit().map_err(sql_error)?;
        Ok(job)
    }
    pub fn fail_job(&mut self, id: &str, error: &JobError) -> Result<StoredJob, String> {
        validate_error(error)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let job = transition_job(&tx, id, &[JobStatus::Running], JobStatus::Failed, |job| {
            job.state.error = Some(error.clone());
        })?;
        tx.commit().map_err(sql_error)?;
        Ok(job)
    }
    pub fn retry_job(&mut self, id: &str) -> Result<StoredJob, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let failed = read_job(&tx, id)?.ok_or(NOT_FOUND)?;
        if failed
            .state
            .error
            .as_ref()
            .is_none_or(|error| !error.retryable)
        {
            return Err("Эту ошибку нельзя повторить с прежним источником.".into());
        }
        let job = transition_job(&tx, id, &[JobStatus::Failed], JobStatus::Queued, |job| {
            job.state.error = None;
        })?;
        tx.commit().map_err(sql_error)?;
        Ok(job)
    }
    pub fn complete_job(&mut self, id: &str) -> Result<StoredJob, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let staged = read_job(&tx, id)?.ok_or(NOT_FOUND)?;
        if staged.status != JobStatus::Running
            || staged.phase != JobPhase::Publishing
            || !staged.state.coverage.complete
            || staged.state.error.is_some()
        {
            return Err("Недопустимый переход состояния задачи импорта.".into());
        }
        let job = transition_job(&tx, id, &[JobStatus::Running], JobStatus::Completed, |_| {})?;
        tx.commit().map_err(sql_error)?;
        Ok(job)
    }
    pub fn commit_job_batch(
        &mut self,
        id: &str,
        expected: &str,
        batch: &JobBatch,
    ) -> Result<StoredJob, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let mut job = read_job(&tx, id)?.ok_or(NOT_FOUND)?;
        if job.status != JobStatus::Running {
            return Err(
                "Задача не выполняется; зафиксировать порцию может только работающая задача."
                    .into(),
            );
        }
        if job.hash != expected {
            return Err(CONFLICT.into());
        }
        apply_batch(&tx, &mut job, batch)?;
        job.hash = job_hash(&job.id, job.status, job.phase, &job.state);
        write_job(&tx, &job)?;
        let restored = read_job(&tx, id)?.ok_or(INVALID)?;
        if restored != job {
            return Err(INVALID.into());
        }
        tx.commit().map_err(sql_error)?;
        Ok(restored)
    }
    /// Jobs left running by a process exit are paused with a retryable
    /// interruption error so a restarted worker can resume them from the last
    /// committed batch without duplicating staged records.
    pub fn recover_interrupted_jobs(&mut self) -> Result<Vec<StoredJob>, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let mut recovered = Vec::new();
        for id in running_job_ids(&tx)? {
            let job = transition_job(&tx, &id, &[JobStatus::Running], JobStatus::Paused, |job| {
                job.state.error = Some(JobError {
                    code: INTERRUPTED.into(),
                    message:
                        "Выполнение прервано перезапуском; задачу можно безопасно возобновить."
                            .into(),
                    retryable: true,
                });
            })?;
            recovered.push(job);
        }
        tx.commit().map_err(sql_error)?;
        Ok(recovered)
    }
    /// Publication reuses the immutable snapshot mechanism: current expected
    /// record generations are verified inside its transaction, an identical
    /// publication receipt is reused, and only a completed job can publish.
    pub fn publish_job(
        &mut self,
        job_id: &str,
        corpus_id: &str,
        publication_id: &str,
    ) -> Result<Snapshot, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let mut job = read_job(&tx, job_id)?.ok_or(NOT_FOUND)?;
        if job.status != JobStatus::Completed
            || !job.state.coverage.complete
            || job.state.error.is_some()
        {
            return Err("Нельзя публиковать незавершённую или отменённую задачу импорта.".into());
        }
        if let Some(snapshot_id) = &job.state.published_snapshot_id {
            let snapshot = read_snapshot(&tx, snapshot_id)?;
            if snapshot.corpus_id != corpus_id || snapshot.publication_id != publication_id {
                return Err("Задача уже опубликована с другим снимком.".into());
            }
            return Ok(snapshot);
        }
        let expected = job_record_references(&tx, job_id)?;
        let snapshot = publish_snapshot(&tx, corpus_id, publication_id, &expected)?;
        job.state.published_snapshot_id = Some(snapshot.id.clone());
        job.hash = job_hash(&job.id, job.status, job.phase, &job.state);
        write_job(&tx, &job)?;
        if read_job(&tx, job_id)?.as_ref() != Some(&job) {
            return Err(INVALID.into());
        }
        tx.commit().map_err(sql_error)?;
        Ok(snapshot)
    }
    pub fn job_record_ids(&self, job_id: &str) -> Result<Vec<String>, String> {
        job_record_ids_in(&self.connection, job_id)
    }
}
