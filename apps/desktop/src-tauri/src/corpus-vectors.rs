//! Durable vector staging and atomic publication in the encrypted corpus.
//!
//! Storage is not a grant authority. The native caller checks model-processing
//! permission before embedding and passes cancellation/revocation checkpoints.
//! Every mutation pins the full current source FTS receipt inside an IMMEDIATE
//! transaction. Retried writes must match exact f32 little-endian bytes; no
//! incomplete index becomes active. Publication streams its ordered manifest,
//! retaining only one bounded vector row, and switches metadata without copying
//! the vector set. Replaying an old publication never rolls back a newer pointer.
//! Source generation deletion cascades through schema v6. Staging cleanup,
//! authorized reads and embedding workers are separate lifecycle responsibilities.
#[path = "corpus-vectors-query.rs"]
mod query;
pub use query::search_vectors;
#[path = "corpus-vectors-read.rs"]
mod read;
use super::search::SearchIndexReceipt;
pub use read::{active_receipt, has_stage_vector};
use rusqlite::Connection;
#[derive(Clone, Debug, PartialEq)]
pub struct VectorStageInput {
    pub stage_id: String,
    pub source: SearchIndexReceipt,
    pub model_fingerprint: String,
    pub dimensions: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorStage {
    pub generation: i64,
    pub vector_count: i64,
    pub published: bool,
}
const INVALID: &str = "Некорректный или устаревший этап векторного индекса.";
fn sql_error(_: rusqlite::Error) -> String {
    "Не удалось выполнить операцию зашифрованного корпуса.".into()
}
fn valid_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
fn validate(input: &VectorStageInput) -> Result<(), String> {
    let source = &input.source;
    if !valid_id(&input.stage_id)
        || !valid_id(&input.model_fingerprint)
        || !(1..=4096).contains(&input.dimensions)
        || !valid_id(&source.snapshot_id)
        || !valid_id(&source.policy_fingerprint)
        || !(1..=super::search::MAX_JS_SAFE_INTEGER).contains(&source.snapshot_generation)
        || !(1..=super::search::MAX_JS_SAFE_INTEGER).contains(&source.index_generation)
        || !(0..=super::search::MAX_STAGE_CHUNKS).contains(&source.chunk_count)
        || source.content_sha256.len() != 64
        || !source.content_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || source.created_at.is_empty()
        || source.created_at.len() > 20
        || !source.created_at.bytes().all(|b| b.is_ascii_digit())
        || source.created_at.parse::<u64>().is_err()
    {
        return Err(INVALID.into());
    }
    Ok(())
}
fn source_json(input: &VectorStageInput) -> Result<String, String> {
    serde_json::to_string(&input.source).map_err(|_| INVALID.into())
}
fn require_source(connection: &Connection, input: &VectorStageInput) -> Result<(), String> {
    if super::search::active_receipt(connection, &input.source.snapshot_id)?.as_ref()
        != Some(&input.source)
    {
        return Err(INVALID.into());
    }
    Ok(())
}
fn load_stage(
    connection: &Connection,
    input: &VectorStageInput,
) -> Result<Option<VectorStage>, String> {
    use rusqlite::OptionalExtension;
    let row = connection.query_row(
        "SELECT generation,vector_count,state,source_receipt_json,model_fingerprint,dimensions,\
         snapshot_id,snapshot_generation,search_generation FROM vector_generations WHERE stage_id=?1",
        [&input.stage_id],
        |r| Ok((r.get::<_,i64>(0)?, r.get::<_,i64>(1)?, r.get::<_,String>(2)?,
            r.get::<_,String>(3)?, r.get::<_,String>(4)?, r.get::<_,i64>(5)?,
            r.get::<_,String>(6)?, r.get::<_,i64>(7)?, r.get::<_,i64>(8)?)),
    ).optional().map_err(sql_error)?;
    let Some((
        generation,
        vector_count,
        state,
        source,
        model,
        dimensions,
        snapshot,
        snapshot_generation,
        search_generation,
    )) = row
    else {
        return Ok(None);
    };
    if snapshot != input.source.snapshot_id
        || snapshot_generation != input.source.snapshot_generation
        || search_generation != input.source.index_generation
        || source != source_json(input)?
        || model != input.model_fingerprint
        || dimensions != input.dimensions as i64
        || !(0..=input.source.chunk_count).contains(&vector_count)
        || !matches!(state.as_str(), "staging" | "published")
    {
        return Err(INVALID.into());
    }
    Ok(Some(VectorStage {
        generation,
        vector_count,
        published: state == "published",
    }))
}
/// Resume the exact stage identity or create an empty durable stage.
pub fn begin_stage(
    connection: &mut Connection,
    input: &VectorStageInput,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<VectorStage, String> {
    check()?;
    validate(input)?;
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    require_source(&tx, input)?;
    let stage = if let Some(existing) = load_stage(&tx, input)? {
        existing
    } else {
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| INVALID.to_string())?
            .as_secs()
            .to_string();
        tx.execute("INSERT INTO vector_generations(stage_id,snapshot_id,snapshot_generation,search_generation,source_receipt_json,model_fingerprint,dimensions,created_at) \
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",rusqlite::params![input.stage_id,input.source.snapshot_id,input.source.snapshot_generation,input.source.index_generation,source_json(input)?,input.model_fingerprint,input.dimensions as i64,created]).map_err(sql_error)?;
        VectorStage {
            generation: tx.last_insert_rowid(),
            vector_count: 0,
            published: false,
        }
    };
    check()?;
    tx.commit().map_err(sql_error)?;
    Ok(stage)
}
fn encode_vector(vector: &[f32], dimensions: usize) -> Result<Vec<u8>, String> {
    if vector.len() != dimensions
        || vector.iter().any(|v| !v.is_finite())
        || vector.iter().all(|v| *v == 0.)
    {
        return Err(INVALID.into());
    }
    Ok(vector.iter().flat_map(|v| v.to_le_bytes()).collect())
}
/// Persist one model-matched source chunk; identical retry is a strict no-op.
pub fn append_vector(
    connection: &mut Connection,
    input: &VectorStageInput,
    chunk_id: &str,
    model_fingerprint: &str,
    vector: &[f32],
    check: &dyn Fn() -> Result<(), String>,
) -> Result<VectorStage, String> {
    use rusqlite::OptionalExtension;
    check()?;
    validate(input)?;
    if !valid_id(chunk_id) || model_fingerprint != input.model_fingerprint {
        return Err(INVALID.into());
    }
    let bytes = encode_vector(vector, input.dimensions)?;
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    require_source(&tx, input)?;
    let mut stage = load_stage(&tx, input)?.ok_or_else(|| INVALID.to_string())?;
    if stage.published {
        return Err(INVALID.into());
    }
    let belongs: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM search_chunks \
        WHERE snapshot_id=?1 AND index_generation=?2 AND chunk_id=?3)",
            rusqlite::params![
                input.source.snapshot_id,
                input.source.index_generation,
                chunk_id
            ],
            |r| r.get(0),
        )
        .map_err(sql_error)?;
    if !belongs {
        return Err(INVALID.into());
    }
    let existing: Option<Vec<u8>> = tx
        .query_row(
            "SELECT vector FROM vector_entries WHERE generation=?1 AND chunk_id=?2",
            rusqlite::params![stage.generation, chunk_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql_error)?;
    if let Some(existing) = existing {
        if existing != bytes {
            return Err(INVALID.into());
        }
    } else {
        if stage.vector_count >= input.source.chunk_count {
            return Err(INVALID.into());
        }
        tx.execute(
            "INSERT INTO vector_entries(generation,chunk_id,vector) VALUES(?1,?2,?3)",
            rusqlite::params![stage.generation, chunk_id, bytes],
        )
        .map_err(sql_error)?;
        tx.execute(
            "UPDATE vector_generations SET vector_count=vector_count+1 WHERE generation=?1",
            [stage.generation],
        )
        .map_err(sql_error)?;
        stage.vector_count += 1;
    }
    check()?;
    tx.commit().map_err(sql_error)?;
    Ok(stage)
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorReceipt {
    pub schema_version: u8,
    pub generation: i64,
    pub source: SearchIndexReceipt,
    pub model_fingerprint: String,
    pub dimensions: usize,
    pub vector_count: i64,
    pub content_sha256: String,
    pub published_at: String,
}
/// Seal a complete generation, checking each row and the final commit boundary.
pub fn publish_stage(
    connection: &mut Connection,
    input: &VectorStageInput,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<VectorReceipt, String> {
    use sha2::{Digest, Sha256};
    check()?;
    validate(input)?;
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    require_source(&tx, input)?;
    let stage = load_stage(&tx, input)?.ok_or_else(|| INVALID.to_string())?;
    if stage.vector_count != input.source.chunk_count {
        return Err(INVALID.into());
    }
    let mut digest = Sha256::new();
    digest.update(b"proteus-vectors:1\0");
    digest.update((input.dimensions as u64).to_le_bytes());
    digest.update((input.model_fingerprint.len() as u64).to_le_bytes());
    digest.update(input.model_fingerprint.as_bytes());
    let mut count = 0i64;
    {
        let mut statement=tx.prepare("SELECT e.chunk_id,e.vector,c.chunk_id FROM vector_entries e \
            LEFT JOIN search_chunks c ON c.snapshot_id=?2 AND c.index_generation=?3 AND c.chunk_id=e.chunk_id \
            WHERE e.generation=?1 ORDER BY e.chunk_id COLLATE BINARY").map_err(sql_error)?;
        let mut rows = statement
            .query(rusqlite::params![
                stage.generation,
                input.source.snapshot_id,
                input.source.index_generation
            ])
            .map_err(sql_error)?;
        while let Some(row) = rows.next().map_err(sql_error)? {
            check()?;
            let id: String = row.get(0).map_err(sql_error)?;
            let bytes: Vec<u8> = row.get(1).map_err(sql_error)?;
            let source_id: Option<String> = row.get(2).map_err(sql_error)?;
            if !valid_id(&id)
                || source_id.as_ref() != Some(&id)
                || bytes.len() != input.dimensions * 4
            {
                return Err(INVALID.into());
            }
            let mut nonzero = false;
            for chunk in bytes.as_chunks::<4>().0 {
                let value = f32::from_le_bytes(*chunk);
                if !value.is_finite() {
                    return Err(INVALID.into());
                }
                nonzero |= value != 0.;
            }
            if !nonzero {
                return Err(INVALID.into());
            }
            count += 1;
            if count > input.source.chunk_count {
                return Err(INVALID.into());
            }
            digest.update((id.len() as u64).to_le_bytes());
            digest.update(id.as_bytes());
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(&bytes);
        }
    }
    if count != stage.vector_count {
        return Err(INVALID.into());
    }
    let content_sha256 = format!("{:x}", digest.finalize());
    let published_at = if stage.published {
        let (stored, published): (String, String) = tx
            .query_row(
                "SELECT content_sha256,published_at FROM vector_generations WHERE generation=?1",
                [stage.generation],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(sql_error)?;
        if stored != content_sha256 || published.parse::<u64>().is_err() {
            return Err(INVALID.into());
        }
        published
    } else {
        let published = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| INVALID.to_string())?
            .as_secs()
            .to_string();
        tx.execute("UPDATE vector_generations SET state='published',content_sha256=?2,published_at=?3 WHERE generation=?1",rusqlite::params![stage.generation,content_sha256,published]).map_err(sql_error)?;
        tx.execute("INSERT INTO vector_active(snapshot_id,generation) VALUES(?1,?2) ON CONFLICT(snapshot_id) \
            DO UPDATE SET generation=excluded.generation",rusqlite::params![input.source.snapshot_id,stage.generation]).map_err(sql_error)?;
        published
    };
    check()?;
    tx.commit().map_err(sql_error)?;
    Ok(VectorReceipt {
        schema_version: 1,
        generation: stage.generation,
        source: input.source.clone(),
        model_fingerprint: input.model_fingerprint.clone(),
        dimensions: input.dimensions,
        vector_count: count,
        content_sha256,
        published_at,
    })
}

impl super::CorpusDatabase {
    pub fn search_vectors(
        &mut self,
        scope: &super::search::SearchScope,
        expected: &VectorReceipt,
        query: &[f32],
        fingerprint: &str,
        budget: crate::local_embedding::search::SearchBudget,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<Vec<crate::local_embedding::search::SemanticHit>, String> {
        search_vectors(
            &mut self.connection,
            scope,
            expected,
            query,
            fingerprint,
            budget,
            check,
        )
    }
    pub fn active_vector_receipt(
        &mut self,
        snapshot_id: &str,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<Option<VectorReceipt>, String> {
        active_receipt(&mut self.connection, snapshot_id, check)
    }
    pub fn has_stage_vector(
        &mut self,
        input: &VectorStageInput,
        chunk_id: &str,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<bool, String> {
        has_stage_vector(&mut self.connection, input, chunk_id, check)
    }
    pub fn begin_vector_stage(
        &mut self,
        input: &VectorStageInput,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<VectorStage, String> {
        begin_stage(&mut self.connection, input, check)
    }
    pub fn append_vector(
        &mut self,
        input: &VectorStageInput,
        chunk_id: &str,
        model_fingerprint: &str,
        vector: &[f32],
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<VectorStage, String> {
        append_vector(
            &mut self.connection,
            input,
            chunk_id,
            model_fingerprint,
            vector,
            check,
        )
    }
    pub fn publish_vector_stage(
        &mut self,
        input: &VectorStageInput,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<VectorReceipt, String> {
        publish_stage(&mut self.connection, input, check)
    }
}

#[cfg(test)]
#[path = "corpus-vectors-tests.rs"]
mod tests;
