//! Bounded native research snapshot storage: durable `MemorySnapshot` /
//! `MemoryItem` bundles pinned to an immutable corpus snapshot.
//!
//! These are host-only storage helpers, not grant authorization and not IPC:
//! there are no generated IDs or timestamps here. The root publication
//! scenario validates original text, authorship, holdout and consumed leases
//! before calling this boundary, and it re-checks live permission before any
//! exposure. Episode identity/reference partition and exact-text permissions
//! likewise remain a host scenario: this slice never infers source truth or
//! UTF8 span alignment from metadata. What is enforced here is structural
//! persistence identity: DTOs validate, the bundle composition is exact, every
//! source span resolves to a `(documentId, revisionId)` pair of the verified
//! immutable corpus snapshot, and stored payloads keep their canonical
//! SHA256/payload counters so corruption is rejected, never repaired.
//!
//! `publish` runs in one IMMEDIATE transaction; a conflicting replay of the
//! same snapshot ID fails instead of overwriting, and an identical replay is
//! idempotent. `read` runs on a named savepoint so it nests inside a caller
//! transaction without committing or rolling back caller work (rusqlite
//! 0.41.1 has no savepoint `TransactionBehavior`; the savepoint is issued in
//! SQL on the private connection, which the serialized host scenario already
//! owns). Native source/snapshot deletion cascades through the schema v7
//! foreign keys.
use super::snapshot_store::read_snapshot_checked;
use super::types::digest;
use super::{sql_error, CorpusDatabase, INVALID};
use crate::memory::types::{
    decode_memory_item, decode_memory_snapshot, MemoryItem, MemorySnapshot,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::collections::HashSet;

const MAX_OWNER_BYTES: usize = 256;
const MAX_ITEM_PAYLOAD_BYTES: i64 = 512 * 1024;
const MAX_SNAPSHOT_PAYLOAD_BYTES: i64 = 1024 * 1024;
const MAX_BUNDLE_PAYLOAD_BYTES: i64 = 16 * 1024 * 1024;
const READ_SAVEPOINT: &str = "proteus_memory_bundle_read";

const MISSING_SNAPSHOT: &str = "Снимок памяти не найден или удалён.";
const MISSING_ITEM: &str = "Элемент памяти не найден или удалён.";
const FOREIGN_OWNER: &str = "Снимок памяти принадлежит другой области персоны.";
const CONFLICTING_SNAPSHOT: &str = "Снимок памяти уже опубликован с другими данными.";
const CONFLICTING_ITEM: &str = "Элемент памяти уже сохранён с другими данными.";
const SHAPE_MISMATCH: &str = "Состав снимка памяти не соответствует элементам.";
const WRONG_SNAPSHOT_BINDING: &str = "Элемент памяти ссылается на другой снимок корпуса.";
const WRONG_EXTRACTION_VERSION: &str = "Версия извлечения элемента не совпадает с версией снимка.";
const SPAN_OUTSIDE_SOURCE: &str = "Интервал источника элемента вне снимка корпуса.";
const UNKNOWN_CONTRADICTION: &str =
    "Ссылка противоречия не входит в публикацию и не существует в области персоны.";
const SNAPSHOT_BUDGET: &str = "Снимок памяти превышает 1 МиБ.";
const ITEM_BUDGET: &str = "Элемент памяти превышает 512 КиБ.";
const BUNDLE_BUDGET: &str = "Память исследования превышает бюджет 16 МиБ.";

/// A verified, immutable research memory bundle: the typed snapshot plus its
/// items in the snapshot's exact `itemIds` order.
#[derive(Clone, Debug, PartialEq)]
pub struct MemoryBundle {
    pub snapshot: MemorySnapshot,
    pub items: Vec<MemoryItem>,
}

// `MemoryItem`/`MemorySnapshot` stop at `PartialEq` in the root-owned DTO
// module; every field of both is an `Eq`-compatible value type, so the
// bundle asserts the structural equality its persistence identity requires.
impl Eq for MemoryBundle {}

#[derive(Clone, Copy)]
enum MemoryRow {
    Snapshot,
    Item,
}

impl MemoryRow {
    fn table(self) -> &'static str {
        match self {
            Self::Snapshot => "memory_snapshots",
            Self::Item => "memory_items",
        }
    }

    fn missing(self) -> &'static str {
        match self {
            Self::Snapshot => MISSING_SNAPSHOT,
            Self::Item => MISSING_ITEM,
        }
    }

    fn cap(self) -> i64 {
        match self {
            Self::Snapshot => MAX_SNAPSHOT_PAYLOAD_BYTES,
            Self::Item => MAX_ITEM_PAYLOAD_BYTES,
        }
    }
}

struct RowMetadata {
    corpus_snapshot_id: String,
    persona_id: String,
    payload_bytes: i64,
    canonical_sha256: String,
}
struct StoredRow {
    metadata: RowMetadata,
    data_json: String,
}

impl CorpusDatabase {
    /// Validates and durably publishes a research memory bundle. The caller
    /// owns authorization; `check` is invoked before, during and before the
    /// final commit, and its exact error aborts the transaction.
    pub fn publish_memory_snapshot_checked(
        &mut self,
        persona_id: &str,
        snapshot: &MemorySnapshot,
        items: &[MemoryItem],
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<MemoryBundle, String> {
        check()?;
        validate_persona_id(persona_id)?;
        validate_bundle_shape(snapshot, items, check)?;
        let snapshot_json = bounded_json(
            snapshot,
            MAX_SNAPSHOT_PAYLOAD_BYTES as usize,
            SNAPSHOT_BUDGET,
        )?;
        let mut remaining = MAX_BUNDLE_PAYLOAD_BYTES as usize - snapshot_json.len();
        let mut item_jsons = Vec::with_capacity(items.len());
        for item in items {
            check()?;
            let cap = remaining.min(MAX_ITEM_PAYLOAD_BYTES as usize);
            let error = if cap < MAX_ITEM_PAYLOAD_BYTES as usize {
                BUNDLE_BUDGET
            } else {
                ITEM_BUDGET
            };
            let json = bounded_json(item, cap, error)?;
            remaining -= json.len();
            item_jsons.push(json);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        check()?;
        // The original corpus snapshot must exist and verify; deletion or
        // corruption of a source never yields access through memory.
        let source = read_snapshot_checked(&tx, &snapshot.corpus_snapshot_id, check)?;
        validate_span_membership(&source, items, check)?;
        check()?;
        validate_contradiction_references(
            &tx,
            &snapshot.corpus_snapshot_id,
            persona_id,
            items,
            check,
        )?;
        check()?;
        let bundle = if snapshot_exists(&tx, &snapshot.id)? {
            // Idempotent replay is allowed only for the exact same bundle and
            // owner; anything else conflicts and must not overwrite.
            let stored = load_bundle(&tx, &snapshot.id, persona_id, check)?;
            if stored.snapshot != *snapshot || stored.items != items {
                return Err(CONFLICTING_SNAPSHOT.into());
            }
            stored
        } else {
            insert_bundle(
                &tx,
                persona_id,
                &snapshot_json,
                &item_jsons,
                snapshot,
                items,
                check,
            )?;
            let stored = load_bundle(&tx, &snapshot.id, persona_id, check)?;
            if stored.snapshot != *snapshot || stored.items != items {
                return Err(INVALID.into());
            }
            stored
        };
        check()?;
        tx.commit().map_err(sql_error)?;
        Ok(bundle)
    }

    /// Materializes a verified bundle. The byte lengths are checked in SQL
    /// before any payload string is materialized; stored counters, canonical
    /// SHA256, DTO schema, owner scope, contiguous membership positions and
    /// the exact itemIds order must all match, and the source snapshot is
    /// revalidated. Runs on a savepoint so a caller transaction is preserved.
    pub fn read_memory_snapshot_checked(
        &self,
        memory_snapshot_id: &str,
        persona_id: &str,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<MemoryBundle, String> {
        check()?;
        validate_persona_id(persona_id)?;
        validate_persona_id(memory_snapshot_id).map_err(|_| INVALID)?;
        self.connection
            .execute_batch(&format!("SAVEPOINT {READ_SAVEPOINT}"))
            .map_err(sql_error)?;
        match load_bundle(&self.connection, memory_snapshot_id, persona_id, check) {
            Ok(bundle) => {
                self.connection
                    .execute_batch(&format!("RELEASE {READ_SAVEPOINT}"))
                    .map_err(sql_error)?;
                check()?;
                Ok(bundle)
            }
            Err(error) => {
                let _ = self.connection.execute_batch(&format!(
                    "ROLLBACK TO {READ_SAVEPOINT}; RELEASE {READ_SAVEPOINT}"
                ));
                Err(error)
            }
        }
    }
}

/// Host owner-ID policy, mirroring the bounded host ids of the DTO module:
/// nonblank, no control characters, at most 256 UTF8 bytes; surrounding
/// whitespace is preserved, never trimmed.
fn validate_persona_id(persona_id: &str) -> Result<(), String> {
    if persona_id.is_empty() || persona_id.chars().all(char::is_whitespace) {
        return Err("Идентификатор персоны не может быть пустым.".into());
    }
    if persona_id.chars().any(char::is_control) {
        return Err("Идентификатор персоны содержит управляющие символы.".into());
    }
    if persona_id.len() > MAX_OWNER_BYTES {
        return Err(format!(
            "Идентификатор персоны превышает {MAX_OWNER_BYTES} байт UTF8."
        ));
    }
    Ok(())
}

fn validate_bundle_shape(
    snapshot: &MemorySnapshot,
    items: &[MemoryItem],
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    snapshot.validate()?;
    if snapshot.item_ids.len() != items.len() {
        return Err(SHAPE_MISMATCH.into());
    }
    for (index, item) in items.iter().enumerate() {
        check()?;
        item.validate()?;
        if item.id != snapshot.item_ids[index] {
            return Err(SHAPE_MISMATCH.into());
        }
        // The item snapshot binding is the immutable corpus snapshot, not the
        // memory snapshot id (mirrors the TS memory contract).
        if item.snapshot_id != snapshot.corpus_snapshot_id {
            return Err(WRONG_SNAPSHOT_BINDING.into());
        }
        if item.extraction_version != snapshot.extraction_version {
            return Err(WRONG_EXTRACTION_VERSION.into());
        }
    }
    Ok(())
}

fn bounded_json(value: &impl Serialize, cap: usize, error: &str) -> Result<String, String> {
    struct Buffer {
        bytes: Vec<u8>,
        cap: usize,
    }
    impl std::io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.cap.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other("memory JSON budget"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Buffer {
        bytes: Vec::new(),
        cap,
    };
    serde_json::to_writer(&mut buffer, value).map_err(|_| error.to_string())?;
    String::from_utf8(buffer.bytes).map_err(|_| INVALID.into())
}

fn validate_span_membership(
    source: &super::Snapshot,
    items: &[MemoryItem],
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    let documents: HashSet<(&str, &str)> = source
        .documents
        .iter()
        .map(|reference| {
            (
                reference.document_id.as_str(),
                reference.revision_id.as_str(),
            )
        })
        .collect();
    for item in items {
        check()?;
        for span in &item.source_spans {
            if !documents.contains(&(span.document_id.as_str(), span.revision_id.as_str())) {
                return Err(SPAN_OUTSIDE_SOURCE.into());
            }
        }
    }
    Ok(())
}

fn validate_contradiction_references(
    connection: &Connection,
    corpus_snapshot_id: &str,
    persona_id: &str,
    items: &[MemoryItem],
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    let batch: HashSet<&str> = items.iter().map(|item| item.id.as_str()).collect();
    let mut query = connection
        .prepare(
            "SELECT 1 FROM memory_items WHERE id=?1 AND corpus_snapshot_id=?2 AND persona_id=?3",
        )
        .map_err(sql_error)?;
    for item in items {
        for contradiction_id in &item.contradiction_ids {
            check()?;
            if batch.contains(contradiction_id.as_str()) {
                continue;
            }
            let found: Option<i64> = query
                .query_row(
                    params![contradiction_id, corpus_snapshot_id, persona_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sql_error)?;
            if found.is_none() {
                return Err(UNKNOWN_CONTRADICTION.into());
            }
        }
    }
    Ok(())
}

/// Reads one stored row, verifying the payload byte length in SQL before the
/// payload string is materialized, then the stored counter and canonical
/// SHA256. Any mismatch is corruption and is rejected, never repaired.
fn read_metadata(
    connection: &Connection,
    kind: MemoryRow,
    id: &str,
) -> Result<RowMetadata, String> {
    let sql = format!(
        "SELECT corpus_snapshot_id,persona_id,payload_bytes,canonical_sha256,\
         length(CAST(data_json AS BLOB)) FROM {} WHERE id=?1",
        kind.table()
    );
    let row = connection
        .query_row(&sql, [id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .optional()
        .map_err(sql_error)?
        .ok_or_else(|| kind.missing().to_string())?;
    let (corpus_snapshot_id, persona_id, payload_bytes, canonical_sha256, stored_bytes) = row;
    if stored_bytes != payload_bytes || !(1..=kind.cap()).contains(&payload_bytes) {
        return Err(INVALID.into());
    }
    Ok(RowMetadata {
        corpus_snapshot_id,
        persona_id,
        payload_bytes,
        canonical_sha256,
    })
}
fn read_payload(
    connection: &Connection,
    kind: MemoryRow,
    id: &str,
    metadata: &RowMetadata,
) -> Result<String, String> {
    let payload_sql = format!("SELECT data_json FROM {} WHERE id=?1", kind.table());
    let data_json: String = connection
        .query_row(&payload_sql, [id], |row| row.get(0))
        .map_err(sql_error)?;
    if data_json.len() as i64 != metadata.payload_bytes
        || digest(data_json.as_bytes()) != metadata.canonical_sha256
    {
        return Err(INVALID.into());
    }
    Ok(data_json)
}
fn read_row(connection: &Connection, kind: MemoryRow, id: &str) -> Result<StoredRow, String> {
    let metadata = read_metadata(connection, kind, id)?;
    let data_json = read_payload(connection, kind, id, &metadata)?;
    Ok(StoredRow {
        metadata,
        data_json,
    })
}

fn read_membership(
    connection: &Connection,
    snapshot_id: &str,
    snapshot: &MemorySnapshot,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Vec<String>, String> {
    let mut query = connection
        .prepare(
            "SELECT position,CASE WHEN length(CAST(item_id AS BLOB)) BETWEEN 1 AND 256 THEN item_id END FROM memory_snapshot_items WHERE snapshot_id=?1 \
             ORDER BY position LIMIT 1001",
        )
        .map_err(sql_error)?;
    let rows = query
        .query_map([snapshot_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    if rows.len() != snapshot.item_ids.len() {
        return Err(INVALID.into());
    }
    for (index, (position, id)) in rows.iter().enumerate() {
        check()?;
        if *position != index as i64 || id.is_none() {
            return Err(INVALID.into());
        }
    }
    let ids: Vec<String> = rows
        .into_iter()
        .map(|(_, item_id)| item_id.ok_or(INVALID))
        .collect::<Result<_, _>>()?;
    if ids != snapshot.item_ids {
        return Err(INVALID.into());
    }
    Ok(ids)
}

/// Loads and fully verifies a bundle: snapshot row, exact membership, per-item
/// counters/scope, the 16 MiB bundle budget before payloads are materialized,
/// per-item DTO/hash/binding checks, and revalidation of the source snapshot.
fn load_bundle(
    connection: &Connection,
    memory_snapshot_id: &str,
    persona_id: &str,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<MemoryBundle, String> {
    check()?;
    let snapshot_row = read_row(connection, MemoryRow::Snapshot, memory_snapshot_id)?;
    if snapshot_row.metadata.persona_id != persona_id {
        return Err(FOREIGN_OWNER.into());
    }
    let snapshot = decode_memory_snapshot(&snapshot_row.data_json)?;
    if snapshot.id != memory_snapshot_id
        || snapshot.corpus_snapshot_id != snapshot_row.metadata.corpus_snapshot_id
    {
        return Err(INVALID.into());
    }
    check()?;
    let membership = read_membership(connection, memory_snapshot_id, &snapshot, check)?;
    check()?;
    // Pass 1: stored counters, owner scope and the bundle budget are checked
    // before any item payload string is materialized.
    let mut total = snapshot_row.metadata.payload_bytes;
    let mut item_rows = Vec::with_capacity(membership.len());
    for item_id in &membership {
        let row = read_metadata(connection, MemoryRow::Item, item_id)?;
        if row.persona_id != persona_id
            || row.corpus_snapshot_id != snapshot_row.metadata.corpus_snapshot_id
        {
            return Err(INVALID.into());
        }
        total += row.payload_bytes;
        if total > MAX_BUNDLE_PAYLOAD_BYTES {
            return Err(INVALID.into());
        }
        item_rows.push(row);
        check()?;
    }
    // Pass 2: payloads are materialized only after every counter passed.
    let mut items = Vec::with_capacity(item_rows.len());
    for (item_id, row) in membership.iter().zip(item_rows) {
        let json = read_payload(connection, MemoryRow::Item, item_id, &row)?;
        let item = decode_memory_item(&json)?;
        if item.id != *item_id
            || item.snapshot_id != snapshot.corpus_snapshot_id
            || item.extraction_version != snapshot.extraction_version
        {
            return Err(INVALID.into());
        }
        items.push(item);
        check()?;
    }
    let source = read_snapshot_checked(connection, &snapshot.corpus_snapshot_id, check)?;
    validate_span_membership(&source, &items, check)?;
    validate_contradiction_references(
        connection,
        &snapshot.corpus_snapshot_id,
        persona_id,
        &items,
        check,
    )?;
    check()?;
    Ok(MemoryBundle { snapshot, items })
}

fn snapshot_exists(connection: &Connection, id: &str) -> Result<bool, String> {
    let found: Option<i64> = connection
        .query_row("SELECT 1 FROM memory_snapshots WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(sql_error)?;
    Ok(found.is_some())
}

fn insert_bundle(
    connection: &Connection,
    persona_id: &str,
    snapshot_json: &str,
    item_jsons: &[String],
    snapshot: &MemorySnapshot,
    items: &[MemoryItem],
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    for (item, json) in items.iter().zip(item_jsons.iter()) {
        check()?;
        if item_exists(connection, &item.id)? {
            // Existing item IDs may be reused only with the exact same DTO in
            // the same persona/corpus scope; anything else conflicts.
            let row = read_row(connection, MemoryRow::Item, &item.id)?;
            let stored = decode_memory_item(&row.data_json)?;
            if stored != *item
                || row.metadata.persona_id != persona_id
                || row.metadata.corpus_snapshot_id != snapshot.corpus_snapshot_id
            {
                return Err(CONFLICTING_ITEM.into());
            }
        } else {
            connection
                .execute(
                    "INSERT INTO memory_items(id,corpus_snapshot_id,persona_id,payload_bytes,data_json,canonical_sha256) \
                     VALUES(?1,?2,?3,?4,?5,?6)",
                    params![
                        item.id,
                        snapshot.corpus_snapshot_id,
                        persona_id,
                        json.len() as i64,
                        json,
                        digest(json.as_bytes())
                    ],
                )
                .map_err(sql_error)?;
        }
    }
    connection
        .execute(
            "INSERT INTO memory_snapshots(id,corpus_snapshot_id,persona_id,payload_bytes,data_json,canonical_sha256) \
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                snapshot.id,
                snapshot.corpus_snapshot_id,
                persona_id,
                snapshot_json.len() as i64,
                snapshot_json,
                digest(snapshot_json.as_bytes())
            ],
        )
        .map_err(sql_error)?;
    for (position, item_id) in snapshot.item_ids.iter().enumerate() {
        check()?;
        connection
            .execute(
                "INSERT INTO memory_snapshot_items(snapshot_id,position,item_id) VALUES(?1,?2,?3)",
                params![snapshot.id, position as i64, item_id],
            )
            .map_err(sql_error)?;
    }
    Ok(())
}

fn item_exists(connection: &Connection, id: &str) -> Result<bool, String> {
    let found: Option<i64> = connection
        .query_row("SELECT 1 FROM memory_items WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(sql_error)?;
    Ok(found.is_some())
}

#[cfg(test)]
#[path = "corpus-memory-store-tests.rs"]
mod tests;
