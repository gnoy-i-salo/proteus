use super::*;
pub(super) fn snapshot(connection: &Connection, id: &str) -> Result<PreparedSnapshot, String> {
    let stored:Option<(String,String,String,String,String)>=connection.query_row("SELECT source_snapshot_id,split_version,tokenizer_version,chunking_version,canonical_sha256 FROM corpus_preparations WHERE snapshot_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(sql_error)?;
    let (source_snapshot_id, split_version, tokenizer_version, chunking_version, hash) =
        stored.ok_or("Подготовленный снимок не найден или отозван.")?;
    let snapshot = read_snapshot(connection, id)?;
    let source = read_snapshot(connection, &source_snapshot_id)?;
    if snapshot.parent_id.as_deref() != Some(source_snapshot_id.as_str())
        || snapshot.records != source.records
        || snapshot.documents != source.documents
        || snapshot.corpus_id != source.corpus_id
    {
        return Err(INVALID.into());
    }
    let episodes=row_digests(connection,id,"SELECT position,episode_id,canonical_sha256 FROM corpus_episodes WHERE snapshot_id=?1 ORDER BY position",100_000)?;
    let chunks=row_digests(connection,id,"SELECT position,chunk_id,canonical_sha256 FROM corpus_prepared_chunks WHERE snapshot_id=?1 ORDER BY position",1_000_000)?;
    if manifest_hash(
        &source_snapshot_id,
        &split_version,
        &tokenizer_version,
        &chunking_version,
        &episodes,
        &chunks,
    )? != hash
    {
        return Err(INVALID.into());
    }
    Ok(PreparedSnapshot {
        snapshot,
        source_snapshot_id,
        split_version,
        tokenizer_version,
        chunking_version,
        episode_ids: episodes.into_iter().map(|(id, _)| id).collect(),
        chunk_count: chunks.len(),
    })
}
fn row_digests(
    connection: &Connection,
    id: &str,
    sql: &str,
    limit: usize,
) -> Result<Vec<(String, String)>, String> {
    let mut query = connection.prepare(sql).map_err(sql_error)?;
    let mut rows = query.query([id]).map_err(sql_error)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(sql_error)? {
        if out.len() >= limit || row.get::<_, i64>(0).map_err(sql_error)? != out.len() as i64 {
            return Err(INVALID.into());
        }
        out.push((
            row.get(1).map_err(sql_error)?,
            row.get(2).map_err(sql_error)?,
        ));
    }
    Ok(out)
}
pub(super) fn episode(
    connection: &Connection,
    snapshot: &PreparedSnapshot,
    id: &str,
) -> Result<PreparedEpisode, String> {
    if !snapshot.episode_ids.iter().any(|e| e == id) {
        return Err("Эпизод не принадлежит выбранному снимку.".into());
    }
    let (json,hash):(String,String)=connection.query_row("SELECT data_json,canonical_sha256 FROM corpus_episodes WHERE snapshot_id=?1 AND episode_id=?2",params![snapshot.snapshot.id,id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(sql_error)?;
    let value: PreparedEpisode = decode(&json, &hash)?;
    let allowed: std::collections::HashSet<_> = snapshot
        .snapshot
        .documents
        .iter()
        .map(|r| (&r.document_id, &r.revision_id))
        .collect();
    if value.schema_version != 1
        || value.id != id
        || value.snapshot_id != snapshot.snapshot.id
        || value.split_version != snapshot.split_version
        || value.assembly_version != "episodes:1"
        || value.document_refs.is_empty()
        || value
            .document_refs
            .iter()
            .any(|r| !allowed.contains(&(&r.document_id, &r.revision_id)))
    {
        return Err(INVALID.into());
    }
    Ok(value)
}
pub(super) fn chunks(
    connection: &Connection,
    id: &str,
    episode_id: &str,
    offset: usize,
    limit: usize,
    max_bytes: usize,
) -> Result<Vec<PreparedChunk>, String> {
    if offset > 1_000_000
        || !(1..=100).contains(&limit)
        || !(1..=16 * 1024 * 1024).contains(&max_bytes)
    {
        return Err("Неверный бюджет чтения фрагментов.".into());
    }
    let snapshot = snapshot(connection, id)?;
    let episode = episode(connection, &snapshot, episode_id)?;
    let allowed: std::collections::HashSet<_> = episode
        .document_refs
        .iter()
        .map(|r| (&r.document_id, &r.revision_id))
        .collect();
    let mut query=connection.prepare("SELECT chunk_id,length(CAST(data_json AS BLOB)),data_json,canonical_sha256 FROM corpus_prepared_chunks WHERE snapshot_id=?1 AND episode_id=?2 ORDER BY position LIMIT ?3 OFFSET ?4").map_err(sql_error)?;
    let mut rows = query
        .query(params![id, episode_id, limit as i64, offset as i64])
        .map_err(sql_error)?;
    let mut bytes = 0usize;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(sql_error)? {
        let size =
            usize::try_from(row.get::<_, i64>(1).map_err(sql_error)?).map_err(|_| INVALID)?;
        bytes = bytes.checked_add(size).ok_or(INVALID)?;
        if bytes > max_bytes {
            return Err("Превышен бюджет чтения фрагментов.".into());
        }
        let json: String = row.get(2).map_err(sql_error)?;
        let hash: String = row.get(3).map_err(sql_error)?;
        let value: PreparedChunk = decode(&json, &hash)?;
        if episode.partition != Partition::Reference
            || value.partition != Partition::Reference
            || value.schema_version != 1
            || value.id != row.get::<_, String>(0).map_err(sql_error)?
            || value.snapshot_id != id
            || value.episode_id != episode_id
            || value.spans.len() != 1
            || value.token_count == 0
            || value.tokenizer_version != snapshot.tokenizer_version
            || value.chunking_version != snapshot.chunking_version
        {
            return Err(INVALID.into());
        }
        let span = &value.spans[0];
        let reference = super::super::RevisionReference {
            document_id: span.document_id.clone(),
            revision_id: span.revision_id.clone(),
        };
        if !allowed.contains(&(&reference.document_id, &reference.revision_id)) {
            return Err(INVALID.into());
        }
        let original = read_revision(connection, &reference)?;
        let original: serde_json::Value =
            serde_json::from_str(&original.document_json).map_err(|_| INVALID)?;
        let text = original
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or(INVALID)?;
        if span.end <= span.start || text.get(span.start..span.end) != Some(value.text.as_str()) {
            return Err(INVALID.into());
        }
        out.push(value);
    }
    Ok(out)
}

pub(super) fn consistent<T>(
    connection: &Connection,
    action: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    connection
        .execute_batch("SAVEPOINT corpus_preparation_read")
        .map_err(sql_error)?;
    match action(connection) {
        Ok(value) => {
            connection
                .execute_batch("RELEASE SAVEPOINT corpus_preparation_read")
                .map_err(sql_error)?;
            Ok(value)
        }
        Err(error) => {
            let _=connection.execute_batch("ROLLBACK TO SAVEPOINT corpus_preparation_read; RELEASE SAVEPOINT corpus_preparation_read");
            Err(error)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_prepared_read_uses_one_wal_snapshot_during_concurrent_revocation() {
        let (mut db, dir, raw, docs) = super::super::tests::setup();
        let prepared = super::super::tests::publish(&mut db, &raw, &docs, "prepared").unwrap();
        let mut other = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
        let result = consistent(&db.connection, |connection| {
            let before = snapshot(connection, &prepared.snapshot.id)?;
            other.remove("record")?;
            let episode = episode(connection, &before, &before.episode_ids[0])?;
            assert_eq!(episode.snapshot_id, before.snapshot.id);
            snapshot(connection, &prepared.snapshot.id)
        });
        assert!(
            result.is_ok(),
            "read mixed states across revocation: {result:?}"
        );
        assert!(db.prepared_snapshot(&prepared.snapshot.id).is_err());
        assert!(db.connection.is_autocommit());
        drop(other);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
