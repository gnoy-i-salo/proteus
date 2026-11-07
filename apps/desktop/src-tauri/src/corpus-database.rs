//! One native canonical corpus database. These storage APIs are not model tools
//! or IPC commands. Sources rechecks current permissions before exposing content.
#[path = "corpus-crypto.rs"]
mod crypto;
#[path = "source-provenance.rs"]
mod provenance;
#[path = "corpus-record-store.rs"]
mod record_store;
#[path = "corpus-snapshot-store.rs"]
mod snapshot_store;
pub use provenance::{Acquisition, HashAlgorithm, ProvenanceLimitation, SourceProvenance};
#[path = "corpus-types.rs"]
mod types;
use rusqlite::{Connection, TransactionBehavior};
use std::path::Path;
pub use types::{
    record_hash, MigrationAudit, RecordGeneration, RecordInput, RevisionInput, RevisionReference,
    Snapshot, StoredRecord, StoredRevision,
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
        if version != 0 && version != 1 {
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
        Ok(Self { connection })
    }
}
#[cfg(test)]
#[path = "corpus-database-tests.rs"]
mod tests;
