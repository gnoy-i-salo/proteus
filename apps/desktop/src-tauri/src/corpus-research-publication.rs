//! One atomic publication of the native research bundle and its audit receipt.
use super::*;
use crate::memory::publication::ResearchPublication;
use std::collections::HashMap;
const MAX_PUBLICATION_BYTES: usize = 16 * 1024 * 1024;
const MISSING: &str = "Квитанция публикации памяти не найдена или удалена.";
const CONFLICT: &str = "Исследование уже опубликовано с другими данными.";
const SAVEPOINT: &str = "proteus_research_publication_read";

#[derive(Clone, Debug, PartialEq)]
pub struct StoredResearchPublication {
    pub publication: ResearchPublication,
    pub bundle: MemoryBundle,
    pub input_sha256: String,
}
impl CorpusDatabase {
    pub fn publish_research_checked(
        &mut self,
        persona_id: &str,
        publication: &ResearchPublication,
        items: &[MemoryItem],
        input_sha256: &str,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<StoredResearchPublication, String> {
        check()?;
        validate_persona_id(persona_id)?;
        validate_hash(input_sha256)?;
        publication.validate()?;
        if publication.input_sha256 != input_sha256 {
            return Err(CONFLICT.into());
        }
        let prepared = prepare_bundle(&publication.receipt.snapshot, items, check)?;
        validate_items(publication, items)?;
        let json = bounded_json(
            publication,
            MAX_PUBLICATION_BYTES,
            "Журнал исследования превышает 16 МиБ.",
        )?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        check()?;
        let bundle = publish_bundle(
            &tx,
            persona_id,
            &publication.receipt.snapshot,
            items,
            &prepared,
            check,
        )?;
        check()?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM memory_publications WHERE id=?1)",
                [&publication.receipt.id],
                |r| r.get(0),
            )
            .map_err(sql_error)?;
        if !exists {
            tx.execute("INSERT INTO memory_publications(id,memory_snapshot_id,input_sha256,payload_bytes,data_json,canonical_sha256) VALUES(?1,?2,?3,?4,?5,?6)", params![publication.receipt.id, publication.receipt.snapshot.id, input_sha256, json.len() as i64, json, digest(json.as_bytes())]).map_err(sql_error)?;
        }
        let stored = load(&tx, &publication.receipt.id, persona_id, check)?;
        if stored.publication != *publication
            || stored.bundle != bundle
            || stored.input_sha256 != input_sha256
        {
            return Err(CONFLICT.into());
        }
        check()?;
        tx.commit().map_err(sql_error)?;
        // No fallible callback after commit: an error must never imply rollback
        // when the immutable publication has already been committed.
        Ok(stored)
    }
    pub fn read_research_publication_checked(
        &self,
        publication_id: &str,
        persona_id: &str,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<StoredResearchPublication, String> {
        check()?;
        validate_persona_id(persona_id)?;
        validate_persona_id(publication_id)?;
        self.connection
            .execute_batch(&format!("SAVEPOINT {SAVEPOINT}"))
            .map_err(sql_error)?;
        let result = load(&self.connection, publication_id, persona_id, check);
        match result {
            Ok(value) => {
                self.connection
                    .execute_batch(&format!("RELEASE {SAVEPOINT}"))
                    .map_err(sql_error)?;
                check()?;
                Ok(value)
            }
            Err(error) => {
                let _ = self
                    .connection
                    .execute_batch(&format!("ROLLBACK TO {SAVEPOINT}; RELEASE {SAVEPOINT}"));
                Err(error)
            }
        }
    }
}

fn validate_hash(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(INVALID.into());
    }
    Ok(())
}
fn validate_items(publication: &ResearchPublication, items: &[MemoryItem]) -> Result<(), String> {
    let receipt = &publication.receipt;
    let used_documents: HashSet<_> = items
        .iter()
        .flat_map(|item| item.source_spans.iter().map(|s| &s.document_id))
        .collect();
    if used_documents.len() as u64 != receipt.coverage.used_documents {
        return Err(INVALID.into());
    }
    // For each revision, keep the greatest end reachable from a *single*
    // original span whose start is at/before this entry. Binary search avoids
    // scanning every audit span for every item; adjacent spans never merge.
    let mut spans: HashMap<(&str, &str), Vec<(usize, usize)>> = HashMap::new();
    for span in &receipt.analyzed_spans {
        spans
            .entry((&span.document_id, &span.revision_id))
            .or_default()
            .push((span.start, span.end));
    }
    for ranges in spans.values_mut() {
        ranges.sort_unstable();
        let mut max_end = 0;
        for (_, end) in ranges {
            max_end = max_end.max(*end);
            *end = max_end;
        }
    }
    for item in items {
        if item.observed_at != receipt.published_at
            || publication
                .retrievals
                .iter()
                .any(|r| r.policy_version != item.policy_version)
            || item.source_spans.iter().any(|span| {
                let Some(ranges) =
                    spans.get(&(span.document_id.as_str(), span.revision_id.as_str()))
                else {
                    return true;
                };
                let position = ranges.partition_point(|(start, _)| *start <= span.start);
                position == 0 || ranges[position - 1].1 < span.end
            })
        {
            return Err(INVALID.into());
        }
    }
    Ok(())
}

fn load(
    connection: &Connection,
    id: &str,
    persona: &str,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<StoredResearchPublication, String> {
    check()?;
    // Inspect counters and bounded metadata before materializing the JSON.
    let row = connection.query_row("SELECT CASE WHEN length(CAST(memory_snapshot_id AS BLOB)) BETWEEN 1 AND 256 THEN memory_snapshot_id END, CASE WHEN length(input_sha256)=64 THEN input_sha256 END, payload_bytes, length(CAST(data_json AS BLOB)), CASE WHEN length(canonical_sha256)=64 THEN canonical_sha256 END FROM memory_publications WHERE id=?1", [id], |r| Ok((r.get::<_,Option<String>>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,r.get::<_,Option<String>>(4)?))).optional().map_err(sql_error)?.ok_or(MISSING)?;
    let (snapshot, input_sha256, size, actual_size, hash) = row;
    if size != actual_size || !(1..=MAX_PUBLICATION_BYTES as i64).contains(&size) {
        return Err(INVALID.into());
    }
    let snapshot = snapshot.ok_or(INVALID)?;
    let input_sha256 = input_sha256.ok_or(INVALID)?;
    let hash = hash.ok_or(INVALID)?;
    validate_hash(&input_sha256)?;
    validate_hash(&hash)?;
    let bundle = load_bundle(connection, &snapshot, persona, check)?;
    check()?;
    let json: String = connection
        .query_row(
            "SELECT data_json FROM memory_publications WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(sql_error)?;
    if json.len() as i64 != size || digest(json.as_bytes()) != hash {
        return Err(INVALID.into());
    }
    let publication: ResearchPublication = serde_json::from_str(&json).map_err(|_| INVALID)?;
    publication.validate().map_err(|_| INVALID)?;
    if publication.input_sha256 != input_sha256
        || publication.receipt.id != id
        || publication.receipt.snapshot != bundle.snapshot
    {
        return Err(INVALID.into());
    }
    validate_items(&publication, &bundle.items).map_err(|_| INVALID)?;
    check()?;
    Ok(StoredResearchPublication {
        publication,
        bundle,
        input_sha256,
    })
}
