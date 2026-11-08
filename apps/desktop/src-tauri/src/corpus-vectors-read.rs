//! Consistent vector publication metadata. Receipts identify data, never grant access.
use super::{sql_error, valid_id, validate, VectorReceipt, VectorStageInput, INVALID};
use crate::corpus_database::search;
use rusqlite::{Connection, OptionalExtension};

/// Return a sealed publication for the current source generation. A rebuilt or
/// unavailable source yields no semantic index; malformed metadata is an error.
/// The savepoint preserves an enclosing caller transaction on every exit.
pub fn active_receipt(
    connection: &mut Connection,
    snapshot_id: &str,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Option<VectorReceipt>, String> {
    check()?;
    if !valid_id(snapshot_id) {
        return Err(INVALID.into());
    }
    let tx = connection.savepoint().map_err(sql_error)?;
    let receipt = load_active(&tx, snapshot_id)?;
    check()?;
    tx.commit().map_err(sql_error)?;
    Ok(receipt)
}
pub(super) fn load_active(
    connection: &Connection,
    snapshot_id: &str,
) -> Result<Option<VectorReceipt>, String> {
    let Some(source) = search::active_receipt(connection, snapshot_id)? else {
        return Ok(None);
    };
    let row = connection
        .query_row(
            "SELECT g.stage_id,g.generation,g.model_fingerprint,g.dimensions,g.vector_count,\
         g.content_sha256,g.published_at,g.source_receipt_json,g.snapshot_generation,\
         g.search_generation,g.state FROM vector_active a JOIN vector_generations g \
         ON g.generation=a.generation AND g.snapshot_id=a.snapshot_id WHERE a.snapshot_id=?1",
            [snapshot_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, i64>(9)?,
                    r.get::<_, String>(10)?,
                ))
            },
        )
        .optional()
        .map_err(sql_error)?;
    let Some((
        stage_id,
        generation,
        model_fingerprint,
        dimensions,
        vector_count,
        hash,
        time,
        encoded,
        snapshot_generation,
        search_generation,
        state,
    )) = row
    else {
        return Ok(None);
    };
    if snapshot_generation != source.snapshot_generation
        || search_generation != source.index_generation
    {
        return Ok(None);
    }
    let dimensions = usize::try_from(dimensions).map_err(|_| INVALID.to_string())?;
    let input = VectorStageInput {
        stage_id,
        source,
        model_fingerprint,
        dimensions,
    };
    validate(&input)?;
    let (Some(content_sha256), Some(published_at)) = (hash, time) else {
        return Err(INVALID.into());
    };
    if state != "published"
        || !(1..=search::MAX_JS_SAFE_INTEGER).contains(&generation)
        || vector_count != input.source.chunk_count
        || encoded != super::source_json(&input)?
        || content_sha256.len() != 64
        || !content_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || published_at.is_empty()
        || published_at.len() > 20
        || !published_at.bytes().all(|b| b.is_ascii_digit())
        || published_at.parse::<u64>().is_err()
    {
        return Err(INVALID.into());
    }
    let actual: i64 = connection
        .query_row(
            "SELECT count(*) FROM vector_entries WHERE generation=?1",
            [generation],
            |r| r.get(0),
        )
        .map_err(sql_error)?;
    if actual != vector_count {
        return Err(INVALID.into());
    }
    Ok(Some(VectorReceipt {
        schema_version: 1,
        generation,
        source: input.source,
        model_fingerprint: input.model_fingerprint,
        dimensions,
        vector_count,
        content_sha256,
        published_at,
    }))
}
