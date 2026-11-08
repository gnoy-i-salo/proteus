//! Recovery listing and discard for abandoned staging rows of the corpus FTS
//! index.
//!
//! These are low-level storage APIs, not an authorization or abandonment
//! authority: the trusted caller identifies abandoned stage IDs (for example
//! a crashed worker that will not resume) and coordinates current owners
//! before workers start. Nothing here decides abandonment by age, never
//! lists or deletes active work on its own, and never touches published
//! receipts, index generations or the active generation pointer.
//! `discard_staging` rechecks the stage state inside its own Immediate
//! transaction, so a stage that was published between listing and discard is
//! a harmless skip, never a deleted publication; a failed write rolls back
//! all cleanup of that call.
use super::{
    ensure_search_schema, invalid_data, sql_error,
    stage::purge_stage_rows,
    types::{invalid_id, MAX_LIMIT},
};
use rusqlite::{Connection, OptionalExtension, ToSql};
use std::collections::HashSet;

/// Lists at most `limit` stage IDs that are still in `staging` state, ordered
/// by `stage_id`. `prefix` is matched literally (no wildcard semantics, no
/// case folding) and `after` is an exclusive keyset cursor for paging.
/// Published stages are never listed, and no staged text or content is read.
pub fn staging_ids(
    connection: &Connection,
    prefix: &str,
    after: Option<&str>,
    limit: usize,
) -> Result<Vec<String>, String> {
    ensure_search_schema(connection)?;
    if invalid_id(prefix) {
        return Err(invalid_data("Недопустимый префикс этапа индекса."));
    }
    if let Some(cursor) = after {
        if invalid_id(cursor) {
            return Err(invalid_data("Недопустимый курсор этапа индекса."));
        }
    }
    if limit == 0 || limit > MAX_LIMIT as usize {
        return Err(invalid_data("Недопустимый размер страницы."));
    }
    // Literal prefix: `stage_id >= prefix` seeks the primary key and
    // `substr(...) = prefix` enforces the exact leading bytes. Binary
    // collation keeps the match case-sensitive, and no LIKE/GLOB pattern is
    // involved, so `%`, `_`, `*` and friends in the prefix stay literal.
    let mut sql = String::from(
        "SELECT stage_id FROM search_stages WHERE state='staging' AND stage_id>=?1 \
         AND substr(stage_id,1,length(?1))=?1",
    );
    let mut values: Vec<Box<dyn ToSql>> = vec![Box::new(prefix.to_string())];
    if let Some(cursor) = after {
        sql.push_str(" AND stage_id>?2");
        values.push(Box::new(cursor.to_string()));
    }
    let limit_slot = values.len() + 1;
    sql.push_str(&format!(" ORDER BY stage_id ASC LIMIT ?{limit_slot}"));
    values.push(Box::new(limit as i64));
    let mut statement = connection.prepare(&sql).map_err(sql_error)?;
    let mut rows = statement
        .query(rusqlite::params_from_iter(
            values.iter().map(|value| value.as_ref()),
        ))
        .map_err(sql_error)?;
    let mut ids = Vec::new();
    while let Some(row) = rows.next().map_err(sql_error)? {
        ids.push(row.get(0).map_err(sql_error)?);
    }
    Ok(ids)
}

/// Discards the requested staging stages in one Immediate transaction: each
/// stage that still is in `staging` state is removed together with its
/// staged chunk and FTS rows. Missing IDs and stages published (or aborted)
/// after listing are harmless skips; published receipts, index generations
/// and the active pointer are never modified. Returns the number of stages
/// actually removed, so retries are idempotent. Any SQL write failure rolls
/// back the whole cleanup.
pub fn discard_staging(connection: &mut Connection, stage_ids: &[String]) -> Result<usize, String> {
    ensure_search_schema(connection)?;
    if stage_ids.is_empty() || stage_ids.len() > MAX_LIMIT as usize {
        return Err(invalid_data("Недопустимый список этапов индекса."));
    }
    let mut seen = HashSet::with_capacity(stage_ids.len());
    for id in stage_ids {
        if invalid_id(id) {
            return Err(invalid_data("Недопустимый идентификатор этапа индекса."));
        }
        if !seen.insert(id.as_str()) {
            return Err(invalid_data("Дублирующийся идентификатор этапа индекса."));
        }
    }
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let mut removed = 0usize;
    for id in stage_ids {
        // Recheck the state inside the write transaction: a stage published
        // between listing and discard is skipped, never deleted, and an
        // already removed stage is an idempotent no-op.
        let state: Option<String> = tx
            .query_row(
                "SELECT state FROM search_stages WHERE stage_id=?1",
                [id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_error)?;
        if state.as_deref() != Some("staging") {
            continue;
        }
        purge_stage_rows(&tx, id)?;
        removed += tx
            .execute("DELETE FROM search_stages WHERE stage_id=?1", [id.as_str()])
            .map_err(sql_error)?;
    }
    tx.commit().map_err(sql_error)?;
    Ok(removed)
}
