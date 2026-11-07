//! One native canonical corpus database. These storage APIs are not model tools
//! or IPC commands. Sources rechecks current permissions before exposing content.
#[path = "corpus-crypto.rs"]
mod crypto;
#[path = "corpus-migrations.rs"]
mod migrations;
#[path = "source-provenance.rs"]
mod provenance;
#[path = "corpus-record-store.rs"]
mod record_store;
#[path = "corpus-revocation.rs"]
mod revocation;
#[path = "corpus-snapshot-store.rs"]
mod snapshot_store;
pub use provenance::{Acquisition, HashAlgorithm, ProvenanceLimitation, SourceProvenance};
#[path = "corpus-job-batch.rs"]
mod job_batch;
#[path = "corpus-job-budget.rs"]
mod job_budget;
#[path = "corpus-job-store.rs"]
mod job_store;
#[path = "corpus-job-types.rs"]
mod job_types;
#[path = "corpus-job-validation.rs"]
mod job_validation;
pub use job_validation::{
    validate_binding as validate_source_binding, validate_budget as validate_ingestion_budget,
};
#[path = "corpus-jobs.rs"]
mod jobs;
#[path = "corpus-types.rs"]
mod types;
pub use job_types::{
    JobBatch, JobBinding, JobBudget, JobCheckpoint, JobCoverage, JobError, JobInput, JobPeriod,
    JobPhase, JobProvider, JobRole, JobSelection, JobState, JobStatus, StoredJob,
};
use rusqlite::{Connection, TransactionBehavior};
use std::path::Path;
pub use types::{
    record_hash, MigrationAudit, RecordGeneration, RecordInput, RecordNamespace, RevisionInput,
    RevisionReference, Snapshot, StoredRecord, StoredRevision,
};
const INVALID: &str = "Сохранённый корпус повреждён или имеет неподдерживаемую версию.";
fn sql_error(_: rusqlite::Error) -> String {
    "Не удалось выполнить операцию зашифрованного корпуса.".into()
}
pub struct CorpusDatabase {
    connection: Connection,
}
impl CorpusDatabase {
    pub(crate) fn open_with_secrets(
        path: &Path,
        secrets: &dyn crate::connections::storage::SecretStore,
    ) -> Result<Self, String> {
        let key = crypto::key(path, secrets)?;
        Self::open(path, &key)
    }
    pub fn open(path: &Path, key: &[u8; 32]) -> Result<Self, String> {
        let mut connection = crypto::open(path, key)?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(sql_error)?;
        if !(0..=3).contains(&version) {
            return Err(INVALID.into());
        }
        if version == 0 {
            let tables:i64=connection.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",[],|row|row.get(0)).map_err(sql_error)?;
            if tables != 0 {
                return Err(INVALID.into());
            }
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(sql_error)?;
            tx.execute_batch(include_str!("corpus-schema.sql"))
                .map_err(sql_error)?;
            tx.commit().map_err(sql_error)?;
        }
        let application: i64 = connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .map_err(sql_error)?;
        if application != 0x50525453 {
            return Err(INVALID.into());
        }
        if version == 1 {
            migrations::upgrade_jobs(&mut connection)?;
        }
        if (1..=2).contains(&version) {
            migrations::upgrade_record_namespaces(&mut connection)?;
        }
        Ok(Self { connection })
    }
}
#[cfg(test)]
#[path = "corpus-jobs-tests.rs"]
mod job_tests;
#[cfg(test)]
#[path = "corpus-namespace-tests.rs"]
mod namespace_tests;
#[cfg(test)]
#[path = "corpus-database-tests.rs"]
mod tests;
