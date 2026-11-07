use super::record_store::{read_record, read_revision};
use super::{sql_error, types::*, CorpusDatabase, INVALID};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::HashSet;
impl CorpusDatabase {
    pub fn publish(
        &mut self,
        corpus_id: &str,
        publication_id: &str,
        expected: &[RecordGeneration],
    ) -> Result<Snapshot, String> {
        if expected.len() > 128 {
            return Err("Неверный состав снимка корпуса.".into());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let snapshot = publish_snapshot(&tx, corpus_id, publication_id, expected)?;
        tx.commit().map_err(sql_error)?;
        Ok(snapshot)
    }
    pub fn snapshot(&self, id: &str) -> Result<Snapshot, String> {
        read_snapshot(&self.connection, id)
    }
}
pub(super) fn publish_snapshot(
    connection: &Connection,
    corpus_id: &str,
    publication_id: &str,
    expected: &[RecordGeneration],
) -> Result<Snapshot, String> {
    if corpus_id.is_empty()
        || corpus_id.len() > 256
        || publication_id.is_empty()
        || publication_id.len() > 256
        || expected.is_empty()
        || expected
            .iter()
            .map(|record| &record.id)
            .collect::<HashSet<_>>()
            .len()
            != expected.len()
    {
        return Err("Неверный состав снимка корпуса.".into());
    }
    let existing: Option<String> = connection
        .query_row(
            "SELECT snapshot_id FROM publication_receipts WHERE corpus_id=?1 AND publication_id=?2",
            params![corpus_id, publication_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)?;
    if let Some(id) = existing {
        let snapshot = read_snapshot(connection, &id)?;
        if snapshot.records != expected {
            return Err("Публикация уже зафиксирована с другим поколением.".into());
        }
        return Ok(snapshot);
    }
    let mut references = Vec::new();
    let mut seen = HashSet::new();
    for expected in expected {
        let record =
            read_record(connection, &expected.id)?.ok_or("Источник снимка отсутствует.")?;
        if record.input.kind == "deleted" || record.generation != expected.generation {
            return Err("Поколение источника изменилось до публикации снимка.".into());
        }
        for reference in record.references {
            if seen.insert(reference.revision_id.clone()) {
                references.push(reference)
            }
        }
    }
    write_snapshot(
        connection,
        corpus_id,
        publication_id,
        expected,
        references,
        None,
    )
}
pub(super) fn fork_snapshot(
    connection: &Connection,
    source: &Snapshot,
    publication_id: &str,
) -> Result<Snapshot, String> {
    if publication_id.is_empty() || publication_id.len() > 256 {
        return Err(INVALID.into());
    }
    write_snapshot(
        connection,
        &source.corpus_id,
        publication_id,
        &source.records,
        source.documents.clone(),
        Some(source.id.clone()),
    )
}
fn write_snapshot(
    connection: &Connection,
    corpus_id: &str,
    publication_id: &str,
    expected: &[RecordGeneration],
    references: Vec<RevisionReference>,
    parent_id: Option<String>,
) -> Result<Snapshot, String> {
    let previous: Option<(String, i64)> = connection
        .query_row(
            "SELECT last_snapshot_id,last_generation FROM corpora WHERE id=?1",
            [corpus_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sql_error)?;
    let generation = match &previous {
        Some((_, generation)) => generation
            .checked_add(1)
            .filter(|value| *value <= 9_007_199_254_740_991)
            .ok_or(INVALID)?,
        None => 1,
    };
    let snapshot = Snapshot {
        id: publication_snapshot_id(corpus_id, publication_id)?,
        corpus_id: corpus_id.into(),
        publication_id: publication_id.into(),
        generation,
        parent_id: parent_id.or_else(|| previous.map(|(id, _)| id)),
        created_at: chrono::Utc::now().to_rfc3339(),
        records: expected.to_vec(),
        documents: references,
    };
    connection.execute("INSERT INTO snapshots(id,corpus_id,publication_id,generation,parent_id,created_at,canonical_sha256) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![snapshot.id,snapshot.corpus_id,snapshot.publication_id,snapshot.generation,snapshot.parent_id,snapshot.created_at,snapshot_hash(&snapshot)]).map_err(sql_error)?;
    connection.execute("INSERT INTO publication_receipts(corpus_id,publication_id,snapshot_id) VALUES(?1,?2,?3)",params![snapshot.corpus_id,snapshot.publication_id,snapshot.id]).map_err(sql_error)?;
    connection.execute("INSERT INTO corpora(id,last_generation,last_snapshot_id) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET last_generation=excluded.last_generation,last_snapshot_id=excluded.last_snapshot_id",params![snapshot.corpus_id,snapshot.generation,snapshot.id]).map_err(sql_error)?;
    for (position, record) in snapshot.records.iter().enumerate() {
        connection.execute("INSERT INTO snapshot_records(snapshot_id,position,record_id,record_generation) VALUES(?1,?2,?3,?4)",params![snapshot.id,position as i64,record.id,record.generation]).map_err(sql_error)?;
    }
    for (position, reference) in snapshot.documents.iter().enumerate() {
        connection
            .execute(
                "INSERT INTO snapshot_revisions(snapshot_id,position,revision_id) VALUES(?1,?2,?3)",
                params![snapshot.id, position as i64, reference.revision_id],
            )
            .map_err(sql_error)?;
    }
    let restored = read_snapshot(connection, &snapshot.id)?;
    if restored != snapshot {
        return Err(INVALID.into());
    }
    Ok(snapshot)
}
pub(super) fn read_snapshot(connection: &Connection, id: &str) -> Result<Snapshot, String> {
    let row:Option<(String,String,i64,Option<String>,String,String)>=connection.query_row("SELECT corpus_id,publication_id,generation,parent_id,created_at,canonical_sha256 FROM snapshots WHERE id=?1",[id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).optional().map_err(sql_error)?;
    let (corpus_id, publication_id, generation, parent_id, created_at, hash) =
        row.ok_or("Снимок корпуса не найден или отозван.")?;
    let mut query=connection.prepare("SELECT record_id,record_generation FROM snapshot_records WHERE snapshot_id=?1 ORDER BY position").map_err(sql_error)?;
    let records = query
        .query_map([id], |row| {
            Ok(RecordGeneration {
                id: row.get(0)?,
                generation: row.get(1)?,
            })
        })
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    // Deletion and corruption never turn a frozen snapshot into access to an old copy.
    for record in &records {
        if read_record(connection, &record.id)?.is_none_or(|record| record.input.kind == "deleted")
        {
            return Err("Источник снимка отозван.".into());
        }
    }
    let mut query=connection.prepare("SELECT revisions.document_id,revisions.id FROM snapshot_revisions JOIN revisions ON revisions.id=snapshot_revisions.revision_id WHERE snapshot_id=?1 ORDER BY position").map_err(sql_error)?;
    let documents = query
        .query_map([id], |row| {
            Ok(RevisionReference {
                document_id: row.get(0)?,
                revision_id: row.get(1)?,
            })
        })
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    for reference in &documents {
        read_revision(connection, reference)?;
    }
    let snapshot = Snapshot {
        id: id.into(),
        corpus_id,
        publication_id,
        generation,
        parent_id,
        created_at,
        records,
        documents,
    };
    if snapshot_hash(&snapshot) != hash {
        return Err(INVALID.into());
    }
    Ok(snapshot)
}
fn snapshot_hash(snapshot: &Snapshot) -> String {
    digest(
        serde_json::to_vec(&(
            &snapshot.id,
            &snapshot.corpus_id,
            &snapshot.publication_id,
            snapshot.generation,
            &snapshot.parent_id,
            &snapshot.created_at,
            snapshot
                .records
                .iter()
                .map(|record| (&record.id, record.generation))
                .collect::<Vec<_>>(),
            &snapshot.documents,
        ))
        .expect("serializing strings cannot fail"),
    )
}

pub(super) fn publication_snapshot_id(
    corpus_id: &str,
    publication_id: &str,
) -> Result<String, String> {
    Ok(format!(
        "snapshot:{}",
        digest(serde_json::to_vec(&(corpus_id, publication_id)).map_err(|_| INVALID)?)
    ))
}
