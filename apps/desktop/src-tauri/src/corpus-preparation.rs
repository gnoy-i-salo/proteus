//! Immutable prepared snapshots in the canonical encrypted database. This layer
//! is not an authorization service; native callers check grants on every access.
#[path = "corpus-preparation-compile.rs"]
mod compile;
#[path = "corpus-preparation-cursor.rs"]
mod cursor;
pub use cursor::{PreparedCursor, PreparedEntry};
#[path = "corpus-preparation-read.rs"]
mod read;
use super::{
    episodes::*,
    record_store::read_revision,
    snapshot_store::{fork_snapshot, publication_snapshot_id, read_snapshot_checked},
    sql_error,
    types::digest,
    CorpusDatabase, Snapshot, INVALID,
};
pub use compile::{CompiledPreparation, VerifiedPreparation};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{de::DeserializeOwned, Serialize};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedSnapshot {
    pub snapshot: Snapshot,
    pub source_snapshot_id: String,
    pub split_version: String,
    pub tokenizer_version: String,
    pub chunking_version: String,
    pub episode_ids: Vec<String>,
    pub chunk_count: usize,
}
pub struct PreparationRequest<'a> {
    pub source_snapshot_id: &'a str,
    pub publication_id: &'a str,
    pub documents: &'a [EpisodeDocument],
    pub split: &'a EpisodePolicy,
    pub tokenizer: &'a dyn EpisodeTokenizer,
    pub chunks: &'a ChunkPolicy,
}
#[derive(Clone)]
struct Row {
    id: String,
    parent: String,
    json: String,
    hash: String,
}
fn row(id: &str, parent: &str, value: &impl Serialize) -> Result<Row, String> {
    let json = serde_json::to_string(value).map_err(|_| INVALID)?;
    let hash = digest(&json);
    Ok(Row {
        id: id.into(),
        parent: parent.into(),
        json,
        hash,
    })
}
fn decode<T: DeserializeOwned>(json: &str, hash: &str) -> Result<T, String> {
    if digest(json) != hash {
        return Err(INVALID.into());
    }
    serde_json::from_str(json).map_err(|_| INVALID.into())
}
fn manifest_hash(
    source: &str,
    split: &str,
    tokenizer: &str,
    chunking: &str,
    episodes: &[(String, String)],
    chunks: &[(String, String)],
) -> Result<String, String> {
    Ok(digest(
        serde_json::to_vec(&(source, split, tokenizer, chunking, episodes, chunks))
            .map_err(|_| INVALID)?,
    ))
}
impl CorpusDatabase {
    /// Metadata only: callers establish ownership before reading any originals.
    pub fn prepared_source_snapshot_id(&self, id: &str) -> Result<String, String> {
        if id.is_empty() || id.len() > 256 {
            return Err(INVALID.into());
        }
        self.connection
            .query_row(
                "SELECT source_snapshot_id FROM corpus_preparations WHERE snapshot_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql_error)?
            .ok_or_else(|| "Подготовленный снимок не найден или отозван.".into())
    }

    pub fn publish_prepared(
        &mut self,
        request: &PreparationRequest<'_>,
    ) -> Result<PreparedSnapshot, String> {
        let verified = self.verify_preparation(request)?;
        let compiled = verified.compile(request.tokenizer, request.chunks)?;
        self.commit_prepared(&compiled)
    }
    pub fn verify_preparation<'a>(
        &self,
        request: &PreparationRequest<'a>,
    ) -> Result<VerifiedPreparation<'a>, String> {
        self.verify_preparation_checked(request, &|| Ok(()))
    }
    pub fn verify_preparation_checked<'a>(
        &self,
        request: &PreparationRequest<'a>,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<VerifiedPreparation<'a>, String> {
        check()?;
        if request.publication_id.is_empty()
            || request.publication_id.len() > 256
            || request.publication_id.chars().any(char::is_control)
        {
            return Err(INVALID.into());
        }
        let source = read_snapshot_checked(&self.connection, request.source_snapshot_id, check)?;
        let id = publication_snapshot_id(&source.corpus_id, request.publication_id)?;
        let prepared = super::episodes::prepare(&id, request.documents, request.split)?;
        // Validate assignment caps/consistency before repeated original reads;
        // verify all exact originals before tokenizing any reference content.
        check()?;
        validate_originals(&self.connection, &source, request.documents, check)?;
        check()?;
        Ok(VerifiedPreparation {
            source,
            id,
            publication_id: request.publication_id.into(),
            prepared,
        })
    }
    pub fn commit_prepared(
        &mut self,
        compiled: &CompiledPreparation,
    ) -> Result<PreparedSnapshot, String> {
        self.commit_prepared_checked(compiled, &|| Ok(()))
    }
    pub fn commit_prepared_checked(
        &mut self,
        compiled: &CompiledPreparation,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<PreparedSnapshot, String> {
        check()?;
        let CompiledPreparation {
            source,
            publication_id,
            id,
            split,
            tokenizer,
            chunking,
            hash,
            episodes,
            chunks,
        } = compiled;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        // A revocation while tokenizing cannot restore any old prepared material.
        if read_snapshot_checked(&tx, &source.id, check)? != *source {
            return Err(INVALID.into());
        }
        let existing:Option<String>=tx.query_row("SELECT snapshot_id FROM publication_receipts WHERE corpus_id=?1 AND publication_id=?2",params![source.corpus_id,publication_id],|r|r.get(0)).optional().map_err(sql_error)?;
        if let Some(existing) = existing {
            let stored: Option<String> = tx
                .query_row(
                    "SELECT canonical_sha256 FROM corpus_preparations WHERE snapshot_id=?1",
                    [&existing],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql_error)?;
            if existing != *id || stored.as_deref() != Some(hash.as_str()) {
                return Err("Публикация уже закреплена за другим составом подготовки.".into());
            }
            check()?;
            let result = read::snapshot(&tx, &existing)?;
            check()?;
            tx.commit().map_err(sql_error)?;
            return Ok(result);
        }
        check()?;
        let snapshot = fork_snapshot(&tx, source, publication_id)?;
        tx.execute("INSERT INTO corpus_preparations(snapshot_id,source_snapshot_id,split_version,tokenizer_version,chunking_version,canonical_sha256) VALUES(?1,?2,?3,?4,?5,?6)",params![snapshot.id,source.id,split,tokenizer,chunking,hash]).map_err(sql_error)?;
        for (position, row) in episodes.iter().enumerate() {
            check()?;
            tx.execute("INSERT INTO corpus_episodes(snapshot_id,episode_id,position,data_json,canonical_sha256) VALUES(?1,?2,?3,?4,?5)",params![snapshot.id,row.id,position as i64,row.json,row.hash]).map_err(sql_error)?;
        }
        for (position, row) in chunks.iter().enumerate() {
            check()?;
            tx.execute("INSERT INTO corpus_prepared_chunks(snapshot_id,chunk_id,episode_id,position,data_json,canonical_sha256) VALUES(?1,?2,?3,?4,?5,?6)",params![snapshot.id,row.id,row.parent,position as i64,row.json,row.hash]).map_err(sql_error)?;
        }
        check()?;
        let result = read::snapshot(&tx, &snapshot.id)?;
        check()?;
        tx.commit().map_err(sql_error)?;
        Ok(result)
    }
    pub fn prepared_snapshot_checked(
        &self,
        id: &str,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<PreparedSnapshot, String> {
        read::consistent(&self.connection, |connection| {
            read::snapshot_checked(connection, id, check)
        })
    }
    pub fn prepared_snapshot(&self, id: &str) -> Result<PreparedSnapshot, String> {
        read::consistent(&self.connection, |connection| {
            read::snapshot(connection, id)
        })
    }
    pub fn prepared_episode(&self, id: &str, episode_id: &str) -> Result<PreparedEpisode, String> {
        read::consistent(&self.connection, |connection| {
            let snapshot = read::snapshot(connection, id)?;
            read::episode(connection, &snapshot, episode_id)
        })
    }
    pub fn prepared_chunks(
        &self,
        id: &str,
        episode_id: &str,
        offset: usize,
        limit: usize,
        max_bytes: usize,
    ) -> Result<Vec<PreparedChunk>, String> {
        read::consistent(&self.connection, |connection| {
            read::chunks(connection, id, episode_id, offset, limit, max_bytes)
        })
    }
}
fn digests(rows: &[Row]) -> Vec<(String, String)> {
    rows.iter()
        .map(|r| (r.id.clone(), r.hash.clone()))
        .collect()
}
fn validate_originals(
    connection: &Connection,
    snapshot: &Snapshot,
    documents: &[EpisodeDocument],
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    let expected: std::collections::BTreeSet<_> = snapshot
        .documents
        .iter()
        .map(|r| (&r.document_id, &r.revision_id))
        .collect();
    let mut actual: std::collections::BTreeSet<_> = documents
        .iter()
        .map(|d| (&d.reference.document_id, &d.reference.revision_id))
        .collect();
    if expected != actual {
        return Err("Подготовка должна учитывать все ревизии исходного снимка.".into());
    }
    for doc in documents {
        check()?;
        // prepare() already checked consistent immutable fields across bindings.
        if !actual.remove(&(&doc.reference.document_id, &doc.reference.revision_id)) {
            continue;
        }
        let original = read_revision(connection, &doc.reference)?;
        let value: serde_json::Value =
            serde_json::from_str(&original.document_json).map_err(|_| INVALID)?;
        if original.source_key != doc.source_key
            || value.get("text").and_then(serde_json::Value::as_str) != Some(doc.text.as_str())
        {
            return Err("Текст подготовки не совпадает с исходной ревизией.".into());
        }
    }
    Ok(())
}
#[cfg(test)]
#[path = "corpus-preparation-tests.rs"]
mod tests;
