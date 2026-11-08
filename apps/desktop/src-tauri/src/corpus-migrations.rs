use super::{sql_error, INVALID};
use rusqlite::{Connection, TransactionBehavior};

/// Schema v1 reserved job tables but never wrote jobs. Unknown populated tables
/// cannot be upgraded by guessing which record generation they referenced.
pub(super) fn upgrade_jobs(connection: &mut Connection) -> Result<(), String> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let jobs: i64 = tx
        .query_row("SELECT count(*) FROM jobs", [], |row| row.get(0))
        .map_err(sql_error)?;
    let records: i64 = tx
        .query_row("SELECT count(*) FROM job_records", [], |row| row.get(0))
        .map_err(sql_error)?;
    if jobs != 0 || records != 0 {
        return Err(INVALID.into());
    }
    tx.execute_batch("ALTER TABLE job_records ADD COLUMN record_generation INTEGER NOT NULL CHECK(record_generation>0); PRAGMA user_version=2;").map_err(sql_error)?;
    tx.commit().map_err(sql_error)
}

/// Preserve every legacy payload/hash; only its owning codec is made explicit.
pub(super) fn upgrade_record_namespaces(connection: &mut Connection) -> Result<(), String> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    tx.execute_batch("ALTER TABLE records ADD COLUMN namespace TEXT NOT NULL DEFAULT 'sourceService' CHECK(namespace IN ('sourceService','ingestion')); CREATE TRIGGER immutable_record_namespace BEFORE UPDATE OF namespace ON records WHEN NEW.namespace<>OLD.namespace BEGIN SELECT RAISE(ABORT,'immutable record namespace'); END; PRAGMA user_version=3;").map_err(sql_error)?;
    tx.commit().map_err(sql_error)
}

/// Derived search data is rebuildable; existing corpus/job bytes are untouched.
pub(super) fn upgrade_search(connection: &mut Connection) -> Result<(), String> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    tx.execute_batch(include_str!("corpus-search-schema.sql"))
        .map_err(sql_error)?;
    tx.pragma_update(None, "user_version", 4)
        .map_err(sql_error)?;
    tx.commit().map_err(sql_error)
}

/// Prepared data is additive; preserve all original and FTS/job bytes.
pub(super) fn upgrade_preparations(connection: &mut Connection) -> Result<(), String> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    tx.execute_batch(include_str!("corpus-preparation-schema.sql"))
        .map_err(sql_error)?;
    tx.pragma_update(None, "user_version", 5)
        .map_err(sql_error)?;
    tx.commit().map_err(sql_error)
}

/// Add recoverable vector generations without rewriting source/FTS/job bytes.
pub(super) fn upgrade_vectors(connection: &mut Connection) -> Result<(), String> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    tx.execute_batch(include_str!("corpus-vector-schema.sql"))
        .map_err(sql_error)?;
    tx.pragma_update(None, "user_version", 6)
        .map_err(sql_error)?;
    tx.commit().map_err(sql_error)
}

/// Research, frozen manifests and simulation share one additive migration.
/// A conflicting object rolls back all new tables; originals stay byte-exact.
pub(super) fn upgrade_memory(connection: &mut Connection) -> Result<(), String> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    tx.execute_batch(include_str!("corpus-memory-schema.sql"))
        .map_err(sql_error)?;
    tx.execute_batch(include_str!("corpus-simulation-schema.sql"))
        .map_err(sql_error)?;
    tx.pragma_update(None, "user_version", 7)
        .map_err(sql_error)?;
    tx.commit().map_err(sql_error)
}
