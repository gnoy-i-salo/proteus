use super::{sql_error, types::*, CorpusDatabase, INVALID};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::HashSet;
impl CorpusDatabase {
    pub fn save_ingestion(&mut self, input: &RecordInput) -> Result<RecordGeneration, String> {
        self.save_verified(input, None, RecordNamespace::Ingestion)
    }
    pub fn ids_in_namespace(&self, namespace: RecordNamespace) -> Result<Vec<String>, String> {
        let mut query = self
            .connection
            .prepare("SELECT id FROM records WHERE namespace=?1 ORDER BY id")
            .map_err(sql_error)?;
        let ids = query
            .query_map([namespace.as_str()], |row| row.get(0))
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?;
        Ok(ids)
    }
    pub fn save(&mut self, input: &RecordInput) -> Result<RecordGeneration, String> {
        self.save_verified(input, None, RecordNamespace::SourceService)
    }
    fn save_verified(
        &mut self,
        input: &RecordInput,
        audit: Option<&MigrationAudit>,
        namespace: RecordNamespace,
    ) -> Result<RecordGeneration, String> {
        validate_input(input)?;
        if let Some(audit) = audit {
            if audit.source_generation > 3
                || audit.document_count != input.documents.len()
                || audit.canonical_sha256 != record_hash(input)
                || audit.ciphertext_sha256.len() != 64
                || !audit
                    .ciphertext_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(INVALID.into());
            }
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let previous = read_record(&tx, &input.id)?;
        if previous.as_ref().is_some_and(|record| {
            record.namespace != namespace
                || record.input.kind == "deleted"
                || record.input.kind != input.kind
                || record.input.provider != input.provider
        }) {
            return Err("Источник отозван или изменил свою область.".into());
        }
        if let Some(previous) = &previous {
            if audit.is_some() && &previous.input != input {
                return Err("Миграция не может заменить уже сохранённое поколение.".into());
            }
            if &previous.input == input {
                if let Some(audit) = audit {
                    write_audit(&tx, &input.id, audit)?;
                }
                let result = RecordGeneration {
                    id: input.id.clone(),
                    generation: previous.generation,
                };
                tx.commit().map_err(sql_error)?;
                return Ok(result);
            }
        }
        let generation = match previous {
            Some(record) => record
                .generation
                .checked_add(1)
                .filter(|value| *value <= 9_007_199_254_740_991)
                .ok_or(INVALID)?,
            None => 1,
        };
        tx.execute("INSERT INTO records(id,kind,provider,state_json,canonical_sha256,generation,namespace) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET state_json=excluded.state_json,canonical_sha256=excluded.canonical_sha256,generation=excluded.generation",params![input.id,input.kind,input.provider,input.state_json,record_hash(input),generation,namespace.as_str()]).map_err(sql_error)?;
        tx.execute(
            "DELETE FROM record_revisions WHERE record_id=?1",
            [&input.id],
        )
        .map_err(sql_error)?;
        for (position, document) in input.documents.iter().enumerate() {
            let id = revision_id(document);
            tx.execute("INSERT OR IGNORE INTO revisions(id,source_key,document_id,document_json,provenance_json,canonical_sha256) VALUES(?1,?2,?3,?4,?5,?6)",params![id,document.source_key,document.document_id,document.document_json,serde_json::to_string(&document.provenance).map_err(|_|INVALID)?,revision_hash(document)]).map_err(sql_error)?;
            let stored = read_revision(
                &tx,
                &RevisionReference {
                    document_id: document.document_id.clone(),
                    revision_id: id.clone(),
                },
            )?;
            if stored.document_json != document.document_json
                || stored.source_key != document.source_key
                || stored.provenance != document.provenance
            {
                return Err(INVALID.into());
            }
            tx.execute("INSERT OR IGNORE INTO record_revision_history(record_id,revision_id) VALUES(?1,?2)",params![input.id,id]).map_err(sql_error)?;
            tx.execute(
                "INSERT INTO record_revisions(record_id,position,revision_id) VALUES(?1,?2,?3)",
                params![input.id, position as i64, id],
            )
            .map_err(sql_error)?;
        }
        let restored = read_record(&tx, &input.id)?.ok_or(INVALID)?;
        if restored.input != *input {
            return Err(INVALID.into());
        }
        if let Some(audit) = audit {
            write_audit(&tx, &input.id, audit)?;
        }
        tx.commit().map_err(sql_error)?;
        Ok(RecordGeneration {
            id: input.id.clone(),
            generation,
        })
    }
    pub fn migrate(
        &mut self,
        input: &RecordInput,
        audit: &MigrationAudit,
    ) -> Result<RecordGeneration, String> {
        self.save_verified(input, Some(audit), RecordNamespace::SourceService)
    }
    pub fn audit(&self, id: &str) -> Result<Option<MigrationAudit>, String> {
        read_audit(&self.connection, id)
    }
    pub fn read(&self, id: &str) -> Result<Option<StoredRecord>, String> {
        read_record(&self.connection, id)
    }
    pub fn ids(&self) -> Result<Vec<String>, String> {
        let mut query = self
            .connection
            .prepare("SELECT id FROM records ORDER BY id")
            .map_err(sql_error)?;
        let ids = query
            .query_map([], |row| row.get(0))
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?;
        Ok(ids)
    }
    pub fn revision(&self, reference: &RevisionReference) -> Result<StoredRevision, String> {
        read_revision(&self.connection, reference)
    }
    pub fn matching_revision(
        &self,
        namespace: RecordNamespace,
        source_key: &str,
        document_json: &str,
    ) -> Result<Option<StoredRevision>, String> {
        let reference = self
            .connection
            .query_row(
                "SELECT document_id,id FROM revisions WHERE source_key=?1 AND document_json=?2
             AND EXISTS(SELECT 1 FROM record_revision_history
                JOIN records ON records.id=record_revision_history.record_id
                WHERE record_revision_history.revision_id=revisions.id
                    AND records.namespace=?3 AND records.kind<>'deleted')
             ORDER BY rowid DESC LIMIT 1",
                params![source_key, document_json, namespace.as_str()],
                |row| {
                    Ok(RevisionReference {
                        document_id: row.get(0)?,
                        revision_id: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(sql_error)?;
        reference
            .map(|reference| read_revision(&self.connection, &reference))
            .transpose()
    }
    pub fn revoke(&mut self, id: &str, provider: &str) -> Result<(), String> {
        if self.read(id)?.is_some() {
            return self.remove(id);
        }
        let tombstone = RecordInput {
            id: id.into(),
            kind: "deleted".into(),
            provider: provider.into(),
            state_json: "{}".into(),
            documents: vec![],
        };
        self.connection.execute("INSERT INTO records(id,kind,provider,state_json,canonical_sha256,generation) VALUES(?1,'deleted',?2,'{}',?3,1)",params![id,provider,record_hash(&tombstone)]).map_err(sql_error)?;
        Ok(())
    }
    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        remove_record(&tx, id)?;
        tx.commit().map_err(sql_error)
    }
}
pub(super) fn remove_record(connection: &Connection, id: &str) -> Result<(), String> {
    let previous = read_record(connection, id)?.ok_or("Источник не найден.")?;
    if previous.input.kind == "deleted" {
        return Ok(());
    }
    let tombstone = RecordInput {
        id: id.into(),
        kind: "deleted".into(),
        provider: previous.input.provider,
        state_json: "{}".into(),
        documents: vec![],
    };
    connection.execute("DELETE FROM snapshots WHERE id IN (SELECT snapshot_id FROM snapshot_records WHERE record_id=?1)",[id]).map_err(sql_error)?;
    connection
        .execute("DELETE FROM record_revisions WHERE record_id=?1", [id])
        .map_err(sql_error)?;
    connection.execute("UPDATE records SET kind='deleted',state_json='{}',canonical_sha256=?2,generation=generation+1 WHERE id=?1",params![id,record_hash(&tombstone)]).map_err(sql_error)?;
    connection
        .execute(
            "DELETE FROM record_revision_history WHERE record_id=?1",
            [id],
        )
        .map_err(sql_error)?;
    connection.execute("DELETE FROM revisions WHERE id NOT IN (SELECT revision_id FROM record_revision_history) AND id NOT IN (SELECT revision_id FROM snapshot_revisions)",[]).map_err(sql_error)?;
    Ok(())
}
fn read_audit(connection: &Connection, id: &str) -> Result<Option<MigrationAudit>, String> {
    connection.query_row("SELECT source_generation,ciphertext_sha256,document_count,canonical_sha256 FROM migration_audits WHERE record_id=?1",[id],|row|Ok(MigrationAudit{source_generation:row.get(0)?,ciphertext_sha256:row.get(1)?,document_count:row.get::<_,i64>(2)? as usize,canonical_sha256:row.get(3)?})).optional().map_err(sql_error)
}
fn write_audit(connection: &Connection, id: &str, audit: &MigrationAudit) -> Result<(), String> {
    if let Some(existing) = read_audit(connection, id)? {
        return if existing == *audit {
            Ok(())
        } else {
            Err(INVALID.into())
        };
    }
    connection.execute("INSERT INTO migration_audits(record_id,source_generation,ciphertext_sha256,canonical_sha256,document_count,migrated_at) VALUES(?1,?2,?3,?4,?5,?6)",params![id,audit.source_generation,audit.ciphertext_sha256,audit.canonical_sha256,audit.document_count as i64,chrono::Utc::now().to_rfc3339()]).map_err(sql_error)?;
    Ok(())
}
fn validate_input(input: &RecordInput) -> Result<(), String> {
    if input.id.is_empty()
        || input.id.len() > 256
        || !matches!(input.kind.as_str(), "grant" | "import")
        || !matches!(
            input.provider.as_str(),
            "telegram" | "vk" | "github" | "instagram" | "web" | "savedPage"
        )
        || input.documents.len() > 10000
        || input.state_json.len()
            + input
                .documents
                .iter()
                .map(|doc| doc.document_json.len())
                .sum::<usize>()
            > 32 * 1024 * 1024
        || input
            .documents
            .iter()
            .map(|doc| &doc.document_id)
            .collect::<HashSet<_>>()
            .len()
            != input.documents.len()
    {
        return Err("Превышен допустимый размер записи корпуса.".into());
    }
    serde_json::from_str::<serde_json::Value>(&input.state_json).map_err(|_| INVALID)?;
    for document in &input.documents {
        document.provenance.validate()?;
        if document.source_key.is_empty()
            || document.source_key.len() > 2048
            || document.document_id.is_empty()
            || document.document_id.len() > 256
        {
            return Err(INVALID.into());
        }
        serde_json::from_str::<serde_json::Value>(&document.document_json).map_err(|_| INVALID)?;
    }
    Ok(())
}
pub(super) fn read_record(
    connection: &Connection,
    id: &str,
) -> Result<Option<StoredRecord>, String> {
    let row: Option<(String, String, String, String, i64, String)> = connection
        .query_row(
            "SELECT kind,provider,state_json,canonical_sha256,generation,namespace FROM records WHERE id=?1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()
        .map_err(sql_error)?;
    let Some((kind, provider, state_json, hash, generation, namespace)) = row else {
        return Ok(None);
    };
    let mut query=connection.prepare("SELECT revisions.document_id,revisions.id FROM record_revisions JOIN revisions ON revisions.id=record_revisions.revision_id WHERE record_id=?1 ORDER BY position").map_err(sql_error)?;
    let references = query
        .query_map([id], |row| {
            Ok(RevisionReference {
                document_id: row.get(0)?,
                revision_id: row.get(1)?,
            })
        })
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    let mut documents = Vec::new();
    for reference in &references {
        let revision = read_revision(connection, reference)?;
        documents.push(RevisionInput {
            source_key: revision.source_key,
            document_id: reference.document_id.clone(),
            document_json: revision.document_json,
            provenance: revision.provenance,
        })
    }
    let input = RecordInput {
        id: id.into(),
        kind,
        provider,
        state_json,
        documents,
    };
    if record_hash(&input) != hash || generation == 0 {
        return Err(INVALID.into());
    }
    Ok(Some(StoredRecord {
        namespace: RecordNamespace::parse(&namespace).ok_or(INVALID)?,
        input,
        generation,
        references,
    }))
}
pub(super) fn read_revision(
    connection: &Connection,
    reference: &RevisionReference,
) -> Result<StoredRevision, String> {
    let row:Option<(String,String,String,String)>=connection.query_row("SELECT source_key,document_json,canonical_sha256,provenance_json FROM revisions WHERE id=?1 AND document_id=?2",params![reference.revision_id,reference.document_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional().map_err(sql_error)?;
    let (source_key, document_json, hash, provenance_json) =
        row.ok_or("Версия источника не найдена.")?;
    let provenance: super::provenance::SourceProvenance =
        serde_json::from_str(&provenance_json).map_err(|_| INVALID)?;
    provenance.validate()?;
    let input = RevisionInput {
        source_key: source_key.clone(),
        document_id: reference.document_id.clone(),
        document_json: document_json.clone(),
        provenance: provenance.clone(),
    };
    if revision_hash(&input) != hash || revision_id(&input) != reference.revision_id {
        return Err(INVALID.into());
    }
    Ok(StoredRevision {
        reference: reference.clone(),
        source_key,
        document_json,
        provenance,
    })
}
