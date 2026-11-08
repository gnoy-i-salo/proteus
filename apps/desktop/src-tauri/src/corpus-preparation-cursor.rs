//! Bounded sequential reading for derived indexes. A cursor pins the immutable
//! manifest, not authorization. Callers recheck current permissions per page and
//! validate the complete snapshot again before publishing a derived index.
use super::*;
use crate::corpus_database::{RecordGeneration, RevisionReference, StoredRevision};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
const BUDGET: &str = "Превышен бюджет чтения подготовленного корпуса.";
pub struct PreparedCursor {
    snapshot: PreparedSnapshot,
    manifest: String,
    chunks: Vec<(String, String)>,
    episodes: HashMap<String, String>,
    originals: HashSet<(String, String)>,
    position: usize,
}
pub struct PreparedEntry {
    pub chunk: PreparedChunk,
    pub episode: Arc<PreparedEpisode>,
    pub original: Arc<StoredRevision>,
    pub source_record: RecordGeneration,
}
impl PreparedCursor {
    pub fn snapshot(&self) -> &PreparedSnapshot {
        &self.snapshot
    }
    pub fn is_complete(&self) -> bool {
        self.position == self.snapshot.chunk_count
    }
    fn check_current(&self, connection: &Connection) -> Result<(), String> {
        let current: Option<(String, i64, String)> = connection.query_row(
            "SELECT p.canonical_sha256,s.generation,p.source_snapshot_id FROM corpus_preparations p JOIN snapshots s ON s.id=p.snapshot_id JOIN snapshots source ON source.id=p.source_snapshot_id WHERE p.snapshot_id=?1",
            [&self.snapshot.snapshot.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).optional().map_err(sql_error)?;
        if current
            != Some((
                self.manifest.clone(),
                self.snapshot.snapshot.generation,
                self.snapshot.source_snapshot_id.clone(),
            ))
        {
            return Err("Подготовленный снимок изменён или отозван.".into());
        }
        Ok(())
    }
}
impl CorpusDatabase {
    pub fn validate_prepared_cursor(
        &self,
        cursor: &PreparedCursor,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<(), String> {
        check()?;
        if !cursor.is_complete() {
            return Err("Чтение подготовленного корпуса не завершено.".into());
        }
        read::consistent(&self.connection, |connection| {
            cursor.check_current(connection)?;
            if read::snapshot_checked(connection, &cursor.snapshot.snapshot.id, check)?
                != cursor.snapshot
            {
                return Err(INVALID.into());
            }
            check()
        })
    }

    pub fn prepared_cursor(
        &self,
        id: &str,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<PreparedCursor, String> {
        check()?;
        read::consistent(&self.connection, |connection| {
            let snapshot = read::snapshot_checked(connection, id, check)?;
            let episodes = read::row_digests(connection, id, "SELECT position,episode_id,canonical_sha256 FROM corpus_episodes WHERE snapshot_id=?1 ORDER BY position", 100_000, check)?;
            let chunks = read::row_digests(connection, id, "SELECT position,chunk_id,canonical_sha256 FROM corpus_prepared_chunks WHERE snapshot_id=?1 ORDER BY position", 1_000_000, check)?;
            let manifest = manifest_hash(
                &snapshot.source_snapshot_id,
                &snapshot.split_version,
                &snapshot.tokenizer_version,
                &snapshot.chunking_version,
                &episodes,
                &chunks,
            )?;
            let originals = snapshot
                .snapshot
                .documents
                .iter()
                .map(|r| (r.document_id.clone(), r.revision_id.clone()))
                .collect();
            check()?;
            Ok(PreparedCursor {
                snapshot,
                manifest,
                chunks,
                episodes: episodes.into_iter().collect(),
                originals,
                position: 0,
            })
        })
    }
    /// Returns a nonempty bounded prefix until exhausted. Byte accounting includes
    /// chunk JSON and each unique episode/original envelope retained in this page.
    /// Cancellation, corruption or insufficient space for one entry never advances.
    pub fn prepared_page(
        &self,
        cursor: &mut PreparedCursor,
        limit: usize,
        max_bytes: usize,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<Vec<PreparedEntry>, String> {
        check()?;
        if !(1..=100).contains(&limit) || !(1..=64 * 1024 * 1024).contains(&max_bytes) {
            return Err(BUDGET.into());
        }
        let page = read::consistent(&self.connection, |connection| {
            cursor.check_current(connection)?;
            let mut cache = PageCache::default();
            let mut out = Vec::new();
            for position in cursor.position..cursor.chunks.len().min(cursor.position + limit) {
                check()?;
                match entry(connection, cursor, position, &mut cache, max_bytes, check)? {
                    Some(value) => out.push(value),
                    None if out.is_empty() => return Err(BUDGET.into()),
                    None => break,
                }
            }
            check()?;
            Ok(out)
        })?;
        check()?;
        cursor.position += page.len();
        Ok(page)
    }
}
#[derive(Default)]
struct PageCache {
    bytes: usize,
    episodes: HashMap<String, Arc<PreparedEpisode>>,
    originals: HashMap<(String, String), Arc<StoredRevision>>,
}
fn reserve(bytes: &mut usize, size: i64, max_bytes: usize) -> Result<bool, String> {
    let size = usize::try_from(size).map_err(|_| INVALID)?;
    let total = bytes.checked_add(size).ok_or(INVALID)?;
    if total > max_bytes {
        return Ok(false);
    }
    *bytes = total;
    Ok(true)
}
fn entry(
    connection: &Connection,
    cursor: &PreparedCursor,
    position: usize,
    cache: &mut PageCache,
    max_bytes: usize,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Option<PreparedEntry>, String> {
    let id = &cursor.snapshot.snapshot.id;
    let (chunk_id, episode_id, size, hash): (String, String, i64, String) = connection.query_row(
        "SELECT chunk_id,episode_id,length(CAST(data_json AS BLOB)),canonical_sha256 FROM corpus_prepared_chunks WHERE snapshot_id=?1 AND position=?2",
        params![id, position as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ).map_err(sql_error)?;
    if cursor.chunks[position] != (chunk_id.clone(), hash.clone()) {
        return Err(INVALID.into());
    }
    if !reserve(&mut cache.bytes, size, max_bytes)? {
        return Ok(None);
    }
    check()?;
    let json: String = connection
        .query_row(
            "SELECT data_json FROM corpus_prepared_chunks WHERE snapshot_id=?1 AND position=?2",
            params![id, position as i64],
            |r| r.get(0),
        )
        .map_err(sql_error)?;
    let chunk: PreparedChunk = decode(&json, &hash)?;
    if chunk.schema_version != 1
        || chunk.id != chunk_id
        || chunk.snapshot_id != *id
        || chunk.episode_id != episode_id
        || chunk.partition != Partition::Reference
        || chunk.spans.len() != 1
        || chunk.token_count == 0
        || chunk.tokenizer_version != cursor.snapshot.tokenizer_version
        || chunk.chunking_version != cursor.snapshot.chunking_version
    {
        return Err(INVALID.into());
    }
    let Some(episode) = read_episode(connection, cursor, &episode_id, cache, max_bytes, check)?
    else {
        return Ok(None);
    };
    let span = &chunk.spans[0];
    let reference = RevisionReference {
        document_id: span.document_id.clone(),
        revision_id: span.revision_id.clone(),
    };
    if !episode.document_refs.contains(&reference) {
        return Err(INVALID.into());
    }
    let Some(original) = read_original(connection, &reference, cache, max_bytes, check)? else {
        return Ok(None);
    };
    let value: serde_json::Value =
        serde_json::from_str(&original.document_json).map_err(|_| INVALID)?;
    let text = value
        .get("text")
        .and_then(serde_json::Value::as_str)
        .ok_or(INVALID)?;
    if span.end <= span.start || text.get(span.start..span.end) != Some(chunk.text.as_str()) {
        return Err(INVALID.into());
    }
    check()?;
    let source_record = connection.query_row(
        "SELECT s.record_id,s.record_generation FROM snapshot_records s JOIN record_revision_history h ON h.record_id=s.record_id WHERE s.snapshot_id=?1 AND h.revision_id=?2 ORDER BY s.position LIMIT 1",
        params![id, reference.revision_id], |r| Ok(RecordGeneration { id: r.get(0)?, generation: r.get(1)? }),
    ).map_err(sql_error)?;
    check()?;
    Ok(Some(PreparedEntry {
        source_record,
        chunk,
        episode,
        original,
    }))
}
fn read_episode(
    connection: &Connection,
    cursor: &PreparedCursor,
    id: &str,
    cache: &mut PageCache,
    max_bytes: usize,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Option<Arc<PreparedEpisode>>, String> {
    if let Some(value) = cache.episodes.get(id) {
        return Ok(Some(Arc::clone(value)));
    }
    check()?;
    let expected = cursor.episodes.get(id).ok_or(INVALID)?;
    let (size, hash): (i64, String) = connection.query_row("SELECT length(CAST(data_json AS BLOB)),canonical_sha256 FROM corpus_episodes WHERE snapshot_id=?1 AND episode_id=?2", params![cursor.snapshot.snapshot.id, id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(sql_error)?;
    if &hash != expected {
        return Err(INVALID.into());
    }
    if !reserve(&mut cache.bytes, size, max_bytes)? {
        return Ok(None);
    }
    check()?;
    let json: String = connection
        .query_row(
            "SELECT data_json FROM corpus_episodes WHERE snapshot_id=?1 AND episode_id=?2",
            params![cursor.snapshot.snapshot.id, id],
            |r| r.get(0),
        )
        .map_err(sql_error)?;
    let value: PreparedEpisode = decode(&json, &hash)?;
    if value.schema_version != 1
        || value.id != id
        || value.snapshot_id != cursor.snapshot.snapshot.id
        || value.partition != Partition::Reference
        || value.split_version != cursor.snapshot.split_version
        || value.assembly_version != "episodes:1"
        || value.document_refs.is_empty()
        || value.document_refs.iter().any(|r| {
            !cursor
                .originals
                .contains(&(r.document_id.clone(), r.revision_id.clone()))
        })
    {
        return Err(INVALID.into());
    }
    let value = Arc::new(value);
    cache.episodes.insert(id.into(), Arc::clone(&value));
    Ok(Some(value))
}
fn read_original(
    connection: &Connection,
    reference: &RevisionReference,
    cache: &mut PageCache,
    max_bytes: usize,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Option<Arc<StoredRevision>>, String> {
    let key = (reference.document_id.clone(), reference.revision_id.clone());
    if let Some(value) = cache.originals.get(&key) {
        return Ok(Some(Arc::clone(value)));
    }
    check()?;
    let size: i64 = connection.query_row("SELECT length(CAST(document_json AS BLOB))+length(CAST(provenance_json AS BLOB))+length(CAST(source_key AS BLOB)) FROM revisions WHERE id=?1 AND document_id=?2", params![reference.revision_id, reference.document_id], |r| r.get(0)).map_err(sql_error)?;
    if !reserve(&mut cache.bytes, size, max_bytes)? {
        return Ok(None);
    }
    check()?;
    let value = Arc::new(read_revision(connection, reference)?);
    cache.originals.insert(key, Arc::clone(&value));
    Ok(Some(value))
}
#[cfg(test)]
#[path = "corpus-preparation-cursor-tests.rs"]
mod tests;
