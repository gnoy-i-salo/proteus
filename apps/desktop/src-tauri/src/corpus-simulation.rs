//! Durable history of generated simulation dialogue in the canonical encrypted
//! database. This namespace is physically separate from source
//! documents/revisions, snapshots, MemoryItems and vector/FTS records; generated
//! text never becomes original-author memory. This layer is storage only, not a
//! model tool or IPC surface, and it verifies structural scope plus manifest
//! existence — native callers recheck live frozen-manifest/source grants through
//! `check` inside transactions and before commit/return.
use super::{sql_error, CorpusDatabase, INVALID};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const TEXT_MAX_BYTES: usize = 64 * 1024;
const ID_MAX_BYTES: usize = 256;
const TIMESTAMP_MAX_BYTES: usize = 64;
const SCOPE_ENTRY_LIMIT: i64 = 1000;
const SCOPE_BYTE_LIMIT: usize = 16 * 1024 * 1024;
/// Conservative valid upper bound for a stored entry payload: 64KiB of text
/// JSON-escapes to ~6x, plus four ≤256-byte opaque ids and a ≤64-byte
/// timestamp. Anything larger cannot be a legitimately written row.
const STORED_JSON_MAX_BYTES: usize = 512 * 1024;
const PAGE_LIMIT_MAX: usize = 100;
const PAGE_BYTES_MAX: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SimulationRole {
    User,
    Simulation,
}

/// Host-orchestrated draft. Native append supplies the persistent `id` and
/// `created_at`; this method is never dispatched from model actions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SimulationMemoryDraft {
    pub schema_version: u8,
    pub experiment_id: String,
    pub manifest_id: String,
    pub trial_id: String,
    pub role: SimulationRole,
    pub text: String,
}

/// Persisted entry; exactly the public contract fields, own camelCase serde.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SimulationMemoryEntry {
    pub schema_version: u8,
    pub id: String,
    pub experiment_id: String,
    pub manifest_id: String,
    pub trial_id: String,
    pub role: SimulationRole,
    pub text: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimulationPageItem {
    pub sequence: i64,
    pub entry: SimulationMemoryEntry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimulationPage {
    pub items: Vec<SimulationPageItem>,
    /// Continuation cursor: sequence of the last returned item when the page
    /// stopped at a boundary — the row limit was reached or the byte budget
    /// cut the page early. It is advisory at an exact scope end: the next
    /// read may legitimately return an empty page. `None` means the scan
    /// consumed the whole scope.
    pub next_after_sequence: Option<i64>,
}

/// Bounded ordered page request over one full scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimulationPageQuery<'a> {
    pub manifest_id: &'a str,
    pub experiment_id: &'a str,
    pub trial_id: &'a str,
    pub after_sequence: i64,
    pub limit: usize,
    pub max_bytes: usize,
}

fn validate_opaque_id(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > ID_MAX_BYTES || value.chars().any(char::is_control)
    {
        return Err("Недопустимый идентификатор записи истории симуляции.".into());
    }
    Ok(())
}

fn validate_scope(experiment_id: &str, manifest_id: &str, trial_id: &str) -> Result<(), String> {
    validate_opaque_id(experiment_id)?;
    validate_opaque_id(manifest_id)?;
    validate_opaque_id(trial_id)?;
    Ok(())
}

fn validate_text(text: &str) -> Result<(), String> {
    if text.is_empty() || text.len() > TEXT_MAX_BYTES {
        return Err("Текст реплики симуляции вне диапазона 1..64KiB.".into());
    }
    Ok(())
}

fn validate_timestamp(created_at: &str) -> Result<(), String> {
    // Explicit small byte bound before chrono parsing: the RFC3339 grammar
    // tolerates arbitrary fractional precision, so without this bound an
    // enormous "timestamp" would parse and defeat the entry size limits.
    if created_at.len() > TIMESTAMP_MAX_BYTES {
        return Err("Метка времени записи превышает допустимый размер.".into());
    }
    if chrono::DateTime::parse_from_rfc3339(created_at).is_err() {
        return Err("Метка времени записи не является корректным RFC3339.".into());
    }
    Ok(())
}

fn validate_draft(draft: &SimulationMemoryDraft) -> Result<(), String> {
    if draft.schema_version != 1 {
        return Err("Неподдерживаемая версия схемы записи симуляции.".into());
    }
    validate_scope(&draft.experiment_id, &draft.manifest_id, &draft.trial_id)?;
    validate_text(&draft.text)?;
    Ok(())
}

fn encode(entry: &SimulationMemoryEntry) -> Result<(String, String, usize), String> {
    let json = serde_json::to_string(entry).map_err(|_| INVALID.to_string())?;
    let hash = format!("{:x}", Sha256::digest(json.as_bytes()));
    let bytes = json.len();
    Ok((json, hash, bytes))
}

fn decode(json: &str, hash: &str) -> Result<SimulationMemoryEntry, String> {
    if format!("{:x}", Sha256::digest(json.as_bytes())) != hash {
        return Err(INVALID.into());
    }
    serde_json::from_str(json).map_err(|_| INVALID.into())
}

/// Bound a stored payload from SQL octet-length metadata before the value is
/// ever materialized as a Rust string.
fn checked_stored_len(octets: i64) -> Result<usize, String> {
    let len = usize::try_from(octets).map_err(|_| INVALID.to_string())?;
    if len == 0 || len > STORED_JSON_MAX_BYTES {
        return Err(INVALID.into());
    }
    Ok(len)
}

/// Fail-closed structural verification of a decoded row against its indexed
/// columns, before any caller sees the text.
fn verify_entry(
    entry: &SimulationMemoryEntry,
    row_id: &str,
    sequence: i64,
    experiment_id: &str,
    manifest_id: &str,
    trial_id: &str,
) -> Result<(), String> {
    if entry.schema_version != 1
        || entry.id != row_id
        || entry.experiment_id != experiment_id
        || entry.manifest_id != manifest_id
        || entry.trial_id != trial_id
        || validate_opaque_id(&entry.id).is_err()
        || validate_scope(&entry.experiment_id, &entry.manifest_id, &entry.trial_id).is_err()
        || validate_text(&entry.text).is_err()
        || validate_timestamp(&entry.created_at).is_err()
        || sequence <= 0
    {
        return Err(INVALID.into());
    }
    Ok(())
}

/// Stored row shape for the idempotent-replay lookup; fields mirror the
/// indexed columns plus SQL octet-length metadata for the payload.
struct StoredSimulationRow {
    experiment_id: String,
    manifest_id: String,
    trial_id: String,
    data_json: String,
    canonical_sha256: String,
    sequence: i64,
}

fn read_consistent<T>(
    connection: &rusqlite::Connection,
    action: impl FnOnce(&rusqlite::Connection) -> Result<T, String>,
) -> Result<T, String> {
    connection
        .execute_batch("SAVEPOINT simulation_read")
        .map_err(sql_error)?;
    match action(connection) {
        Ok(value) => {
            connection
                .execute_batch("RELEASE SAVEPOINT simulation_read")
                .map_err(sql_error)?;
            Ok(value)
        }
        Err(error) => {
            let _ = connection.execute_batch(
                "ROLLBACK TO SAVEPOINT simulation_read; RELEASE SAVEPOINT simulation_read",
            );
            Err(error)
        }
    }
}

impl CorpusDatabase {
    /// Atomic immediate-transaction append. The host supplies the persistent `id`
    /// and `created_at`; `check` revalidates live grants and runs inside the
    /// transaction and before the final commit.
    pub fn append_simulation_checked(
        &mut self,
        draft: &SimulationMemoryDraft,
        id: &str,
        created_at: &str,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<SimulationMemoryEntry, String> {
        check()?;
        validate_draft(draft)?;
        validate_opaque_id(id)?;
        validate_timestamp(created_at)?;
        let entry = SimulationMemoryEntry {
            schema_version: 1,
            id: id.into(),
            experiment_id: draft.experiment_id.clone(),
            manifest_id: draft.manifest_id.clone(),
            trial_id: draft.trial_id.clone(),
            role: draft.role,
            text: draft.text.clone(),
            created_at: created_at.into(),
        };
        let (json, hash, bytes) = encode(&entry)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        check()?;
        let manifest_exists: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM memory_manifests WHERE id=?1",
                [&entry.manifest_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_error)?;
        if manifest_exists.is_none() {
            return Err("Манифест памяти не найден.".into());
        }
        let existing: Option<Result<StoredSimulationRow, String>> = tx
            .query_row(
                "SELECT experiment_id,manifest_id,trial_id,data_json,canonical_sha256,sequence,\
                 payload_bytes,length(CAST(data_json AS BLOB)) \
                 FROM simulation_entries WHERE id=?1",
                [&entry.id],
                |row| {
                    let json_octets = row.get::<_, i64>(7)?;
                    let payload_bytes = row.get::<_, i64>(6)?;
                    if checked_stored_len(json_octets).is_err() || payload_bytes != json_octets {
                        return Ok(Err(INVALID.into()));
                    }
                    Ok(Ok(StoredSimulationRow {
                        experiment_id: row.get(0)?,
                        manifest_id: row.get(1)?,
                        trial_id: row.get(2)?,
                        data_json: row.get(3)?,
                        canonical_sha256: row.get(4)?,
                        sequence: row.get(5)?,
                    }))
                },
            )
            .optional()
            .map_err(sql_error)?;
        let existing = existing.transpose()?;
        if let Some(stored_row) = existing {
            check()?;
            let stored = decode(&stored_row.data_json, &stored_row.canonical_sha256)?;
            if stored_row.experiment_id != entry.experiment_id
                || stored_row.manifest_id != entry.manifest_id
                || stored_row.trial_id != entry.trial_id
                || stored_row.canonical_sha256 != hash
                || stored != entry
            {
                return Err(
                    "Конфликтующая запись истории симуляции с таким идентификатором уже существует."
                        .into(),
                );
            }
            verify_entry(
                &stored,
                id,
                stored_row.sequence,
                &entry.experiment_id,
                &entry.manifest_id,
                &entry.trial_id,
            )?;
            // Recheck live grants immediately before exposing stored text.
            check()?;
            return Ok(stored);
        }
        check()?;
        // Capacity uses the actual serialized payload octets. The indexed
        // counters must agree with them — tampered counters fail closed
        // instead of admitting appends beyond the real cap. Bounded work:
        // SQL sums and lengths only, never the scope text in memory.
        let (count, counter_bytes, actual_bytes): (i64, i64, i64) = tx
            .query_row(
                "SELECT count(*), coalesce(sum(payload_bytes),0), \
                 coalesce(sum(length(CAST(data_json AS BLOB))),0) FROM simulation_entries \
                 WHERE manifest_id=?1 AND experiment_id=?2 AND trial_id=?3",
                params![&entry.manifest_id, &entry.experiment_id, &entry.trial_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(sql_error)?;
        if counter_bytes != actual_bytes {
            return Err(INVALID.into());
        }
        if count >= SCOPE_ENTRY_LIMIT {
            return Err("Достигнут лимит записей истории для области симуляции.".into());
        }
        let total = usize::try_from(actual_bytes).map_err(|_| INVALID.to_string())?;
        let next_total = total
            .checked_add(bytes)
            .ok_or_else(|| INVALID.to_string())?;
        if next_total > SCOPE_BYTE_LIMIT {
            return Err("Достигнут объёмный лимит истории для области симуляции.".into());
        }
        let sequence = count + 1;
        check()?;
        tx.execute(
            "INSERT INTO simulation_entries \
             (id,experiment_id,manifest_id,trial_id,sequence,payload_bytes,data_json,canonical_sha256) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                &entry.id,
                &entry.experiment_id,
                &entry.manifest_id,
                &entry.trial_id,
                sequence,
                bytes as i64,
                &json,
                &hash
            ],
        )
        .map_err(sql_error)?;
        check()?;
        tx.commit().map_err(sql_error)?;
        Ok(entry)
    }

    pub fn append_simulation(
        &mut self,
        draft: &SimulationMemoryDraft,
        id: &str,
        created_at: &str,
    ) -> Result<SimulationMemoryEntry, String> {
        self.append_simulation_checked(draft, id, created_at, &|| Ok(()))
    }

    /// Bounded ordered page by full scope. `check` revalidates live grants while
    /// reading; a revoked or cancelled callback exposes no text.
    pub fn simulation_page_checked(
        &self,
        query: &SimulationPageQuery<'_>,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<SimulationPage, String> {
        validate_scope(query.experiment_id, query.manifest_id, query.trial_id)?;
        if query.after_sequence < 0 {
            return Err("Недопустимый курсор чтения истории симуляции.".into());
        }
        if !(1..=PAGE_LIMIT_MAX).contains(&query.limit) {
            return Err("Недопустимый лимит чтения истории симуляции.".into());
        }
        if !(1..=PAGE_BYTES_MAX).contains(&query.max_bytes) {
            return Err("Недопустимый байтовый бюджет чтения истории симуляции.".into());
        }
        check()?;
        read_consistent(&self.connection, |connection| {
            // The manifest must exist in this read snapshot; FK cascades alone
            // are not enough (orphaned rows must never expose text).
            let manifest_exists: Option<i64> = connection
                .query_row(
                    "SELECT 1 FROM memory_manifests WHERE id=?1",
                    [query.manifest_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sql_error)?;
            if manifest_exists.is_none() {
                check()?;
                return Ok(SimulationPage {
                    items: Vec::new(),
                    next_after_sequence: None,
                });
            }
            let mut statement = connection
                .prepare(
                    "SELECT sequence,payload_bytes,data_json,canonical_sha256,id,\
                     length(CAST(data_json AS BLOB)) \
                     FROM simulation_entries \
                     WHERE manifest_id=?1 AND experiment_id=?2 AND trial_id=?3 AND sequence>?4 \
                     ORDER BY sequence LIMIT ?5",
                )
                .map_err(sql_error)?;
            let mut rows = statement
                .query(params![
                    query.manifest_id,
                    query.experiment_id,
                    query.trial_id,
                    query.after_sequence,
                    query.limit as i64
                ])
                .map_err(sql_error)?;
            let mut items = Vec::new();
            let mut bytes = 0usize;
            let mut stopped_by_budget = false;
            while let Some(row) = rows.next().map_err(sql_error)? {
                check()?;
                let sequence = row.get::<_, i64>(0).map_err(sql_error)?;
                let payload_bytes_i64 = row.get::<_, i64>(1).map_err(sql_error)?;
                let json_octets = row.get::<_, i64>(5).map_err(sql_error)?;
                // Bound the payload from SQL octet metadata and require the
                // indexed byte counter to agree before materializing the text.
                let stored_len = checked_stored_len(json_octets)?;
                let payload_bytes =
                    usize::try_from(payload_bytes_i64).map_err(|_| INVALID.to_string())?;
                if payload_bytes != stored_len {
                    return Err(INVALID.into());
                }
                let json = row.get::<_, String>(2).map_err(sql_error)?;
                let hash = row.get::<_, String>(3).map_err(sql_error)?;
                let row_id = row.get::<_, String>(4).map_err(sql_error)?;
                let entry = decode(&json, &hash)?;
                verify_entry(
                    &entry,
                    &row_id,
                    sequence,
                    query.experiment_id,
                    query.manifest_id,
                    query.trial_id,
                )?;
                let expected = query
                    .after_sequence
                    .checked_add(items.len() as i64 + 1)
                    .ok_or_else(|| INVALID.to_string())?;
                if sequence != expected {
                    return Err(INVALID.into());
                }
                let next_bytes = bytes
                    .checked_add(payload_bytes)
                    .ok_or_else(|| INVALID.to_string())?;
                if next_bytes > query.max_bytes {
                    if items.is_empty() {
                        return Err("Первая запись истории не помещается в байтовый бюджет.".into());
                    }
                    stopped_by_budget = true;
                    break;
                }
                bytes = next_bytes;
                items.push(SimulationPageItem { sequence, entry });
            }
            // Continuation must point at the last returned row when the page
            // stopped at a boundary — the row limit or the byte budget. At an
            // exact scope end the cursor is advisory: one extra empty read is
            // expected. `None` only when the scan consumed the whole scope.
            let next_after_sequence = if stopped_by_budget || items.len() == query.limit {
                items.last().map(|item| item.sequence)
            } else {
                None
            };
            check()?;
            Ok(SimulationPage {
                items,
                next_after_sequence,
            })
        })
    }

    pub fn simulation_page(
        &self,
        query: &SimulationPageQuery<'_>,
    ) -> Result<SimulationPage, String> {
        self.simulation_page_checked(query, &|| Ok(()))
    }
}
#[cfg(test)]
#[path = "corpus-simulation-tests.rs"]
mod tests;

#[cfg(test)]
#[path = "corpus-simulation-encrypted-tests.rs"]
mod encrypted_tests;
