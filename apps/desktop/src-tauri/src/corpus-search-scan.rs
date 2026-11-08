//! Bounded reference-chunk scanning over the encrypted corpus FTS index.
//!
//! `scan_reference_chunks` enumerates the authorized `reference` chunks of
//! one pinned publication: no query and no ranking — every in-scope chunk is
//! a candidate, so downstream semantic retrieval is never restricted to
//! lexical hits. Rows stream in deterministic bytewise `chunk_id` order under
//! exact chunk-id keyset pagination.
//!
//! Contract:
//! - the caller-authorized scope bounds every row; this helper is not an
//!   authorization authority and never widens grants — the caller must
//!   revalidate current permissions before any model call;
//! - the scope filter is shared with the FTS hit query (`scope`), never
//!   copied, so per-persona/grant/binding/period semantics stay identical;
//! - the request scope must describe the receipt snapshot, and the full
//!   active publication receipt must equal `expected` inside one pinned read
//!   snapshot (a SAVEPOINT that nests in a caller transaction): a rebuild, a
//!   prune, a policy/content/count drift or a deleted/advanced snapshot is an
//!   explicit error, never a stale page;
//! - a page holds at most [`MAX_SCAN_LIMIT`] rows and at most
//!   [`MAX_SCAN_TEXT_BYTES`] indexed-text bytes; the first chunk must fit the
//!   byte budget or the scan fails, and a page that already holds rows stops
//!   with `has_more` and the last successful cursor instead of skipping rows;
//! - chunk text is read per row (staging bounds a chunk by `MAX_CHUNK_BYTES`
//!   in `types`), the scan never materializes the corpus, and a full page
//!   probes remaining rows with a metadata-only `EXISTS` check instead of
//!   reading one extra chunk text;
//! - `checkpoint` runs before any work, before each row read and once after
//!   the page is assembled; its error is returned unchanged and a failed scan
//!   releases the pinned snapshot even when nested in a caller transaction.
use super::{
    ensure_search_schema, invalid_data,
    read::{require_receipt, validate_receipt_identity, with_search_savepoint},
    scope::{build_scope_filter, slot},
    sql_error,
    types::{invalid_id, validate_scope, SearchIndexReceipt, SearchScope, MAX_LIMIT},
};
use rusqlite::{Connection, OptionalExtension, ToSql};

/// Maximum number of chunks in one scan page.
pub const MAX_SCAN_LIMIT: usize = MAX_LIMIT as usize;
/// Maximum total indexed-text bytes across one scan page (64 MiB).
pub const MAX_SCAN_TEXT_BYTES: usize = 64 * 1024 * 1024;

/// One authorized reference chunk with its exact indexed text and source
/// span. There is deliberately no score: scanning is not ranking.
#[derive(Clone, Debug, PartialEq)]
pub struct ReferenceChunk {
    pub chunk_id: String,
    pub document_id: String,
    pub revision_id: String,
    pub span_start: i64,
    pub span_end: i64,
    pub persona_id: String,
    pub grant_id: String,
    pub binding_id: String,
    pub period_from: Option<i64>,
    pub period_to: Option<i64>,
    pub conversation_id: Option<String>,
    pub thread_id: Option<String>,
    pub text: String,
}

/// One bounded scan page. `next_cursor` is the last successfully returned
/// chunk id and is set exactly when `has_more` is true.
#[derive(Clone, Debug, PartialEq)]
pub struct ReferenceChunkPage {
    pub snapshot_id: String,
    pub snapshot_generation: i64,
    pub index_generation: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
    pub chunks: Vec<ReferenceChunk>,
}

/// Scans authorized reference chunks of the pinned publication. See the
/// module contract; this is an enumeration primitive for retrieval, not an
/// authorization decision and not a lexical query.
pub fn scan_reference_chunks(
    connection: &Connection,
    scope: &SearchScope,
    expected: &SearchIndexReceipt,
    after: Option<&str>,
    limit: usize,
    max_text_bytes: usize,
    checkpoint: &dyn Fn() -> Result<(), String>,
) -> Result<ReferenceChunkPage, String> {
    checkpoint()?;
    ensure_search_schema(connection)?;
    validate_scope(scope)?;
    validate_scan_page(after, limit, max_text_bytes)?;
    validate_receipt_identity(expected)?;
    if scope.snapshot_id != expected.snapshot_id
        || scope.snapshot_generation != expected.snapshot_generation
    {
        return Err(invalid_data(
            "Область сканирования должна указывать снимок квитанции индекса.",
        ));
    }
    with_search_savepoint(connection, || {
        scan_pinned(
            connection,
            scope,
            expected,
            after,
            limit,
            max_text_bytes,
            checkpoint,
        )
    })
}

fn validate_scan_page(
    after: Option<&str>,
    limit: usize,
    max_text_bytes: usize,
) -> Result<(), String> {
    if limit == 0
        || limit > MAX_SCAN_LIMIT
        || max_text_bytes == 0
        || max_text_bytes > MAX_SCAN_TEXT_BYTES
    {
        return Err(invalid_data("Недопустимые ограничения сканирования."));
    }
    if after.is_some_and(invalid_id) {
        return Err(invalid_data("Недопустимый курсор сканирования."));
    }
    Ok(())
}

fn scan_pinned(
    connection: &Connection,
    scope: &SearchScope,
    expected: &SearchIndexReceipt,
    after: Option<&str>,
    limit: usize,
    max_text_bytes: usize,
    checkpoint: &dyn Fn() -> Result<(), String>,
) -> Result<ReferenceChunkPage, String> {
    let actual = require_receipt(connection, scope, expected)?;
    let (chunks, has_more) = collect_chunks(
        connection,
        scope,
        actual.index_generation,
        after,
        limit,
        max_text_bytes,
        checkpoint,
    )?;
    // Cooperative cancellation after the work; the error is preserved.
    checkpoint()?;
    let next_cursor = has_more.then(|| {
        chunks
            .last()
            .expect("has_more implies a nonempty page")
            .chunk_id
            .clone()
    });
    Ok(ReferenceChunkPage {
        snapshot_id: actual.snapshot_id,
        snapshot_generation: actual.snapshot_generation,
        index_generation: actual.index_generation,
        has_more,
        next_cursor,
        chunks,
    })
}

/// Streams rows in bytewise `chunk_id` order and applies the row/text bounds.
/// Stops without reading further rows once the page is full or the byte
/// budget is exhausted; a full page detects remaining rows with a
/// metadata-only `EXISTS` probe.
fn collect_chunks(
    connection: &Connection,
    scope: &SearchScope,
    index_generation: i64,
    after: Option<&str>,
    limit: usize,
    max_text_bytes: usize,
    checkpoint: &dyn Fn() -> Result<(), String>,
) -> Result<(Vec<ReferenceChunk>, bool), String> {
    let mut next = 0i64;
    let filter = build_scope_filter(scope, index_generation, &mut next);
    let filter_sql = filter.sql;
    let after_clause = after
        .map(|_| format!(" AND c.chunk_id>?{}", slot(&mut next)))
        .unwrap_or_default();
    let limit_slot = slot(&mut next);
    // Sort only bounded metadata. Including f.text here could make SQLite's
    // sorter materialize large source bodies before the caller's byte gate.
    let sql = format!(
        "SELECT {REFERENCE_COLUMNS} \
         FROM search_chunks c WHERE {filter_sql}{after_clause} \
         ORDER BY c.chunk_id COLLATE BINARY ASC LIMIT ?{limit_slot}"
    );
    let mut values: Vec<Box<dyn ToSql>> = filter.params;
    if let Some(cursor) = after {
        values.push(Box::new(cursor.to_string()));
    }
    values.push(Box::new(limit as i64));
    let mut statement = connection.prepare(&sql).map_err(sql_error)?;
    let mut rows = statement
        .query(rusqlite::params_from_iter(
            values.iter().map(|value| value.as_ref()),
        ))
        .map_err(sql_error)?;
    let mut chunks: Vec<ReferenceChunk> = Vec::new();
    let mut used_bytes = 0usize;
    let mut stopped_at_budget = false;
    loop {
        if chunks.len() == limit {
            break;
        }
        checkpoint()?;
        let Some(row) = rows.next().map_err(sql_error)? else {
            break;
        };
        let row_id: i64 = row.get(12).map_err(sql_error)?;
        let Some(text) = bounded_text(connection, row_id, max_text_bytes - used_bytes)? else {
            if chunks.is_empty() {
                return Err(invalid_data(
                    "Текст чанка превышает байтовый бюджет сканирования.",
                ));
            }
            stopped_at_budget = true;
            break;
        };
        used_bytes += text.len();
        chunks.push(reference_chunk(row, text)?);
    }
    drop(rows);
    drop(statement);
    if stopped_at_budget {
        return Ok((chunks, true));
    }
    if chunks.len() == limit {
        let more = has_more_rows(connection, scope, index_generation, &chunks)?;
        return Ok((chunks, more));
    }
    Ok((chunks, false))
}

/// Metadata-only probe for a row behind the last returned chunk id. Any row
/// of the full page sorts after the original `after` cursor as well, so the
/// keyset bound is fully described by the last returned chunk id.
fn has_more_rows(
    connection: &Connection,
    scope: &SearchScope,
    index_generation: i64,
    chunks: &[ReferenceChunk],
) -> Result<bool, String> {
    let Some(last) = chunks.last() else {
        return Ok(false);
    };
    let mut next = 0i64;
    let filter = build_scope_filter(scope, index_generation, &mut next);
    let filter_sql = filter.sql;
    let last_slot = slot(&mut next);
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM search_chunks c WHERE {filter_sql} AND c.chunk_id>?{last_slot})"
    );
    let mut values: Vec<Box<dyn ToSql>> = filter.params;
    values.push(Box::new(last.chunk_id.clone()));
    connection
        .query_row(
            &sql,
            rusqlite::params_from_iter(values.iter().map(|value| value.as_ref())),
            |row| row.get::<_, i64>(0),
        )
        .map_err(sql_error)
        .map(|found| found != 0)
}

/// Read the length before exposing a string to Rust. The CASE keeps an
/// over-budget next chunk out of the result and enforces the per-chunk cap
/// even if stored FTS text has been damaged outside the normal writer.
pub(super) fn bounded_text(
    connection: &Connection,
    row_id: i64,
    remaining: usize,
) -> Result<Option<String>, String> {
    let value = connection
        .query_row(
            "SELECT length(CAST(text AS BLOB)), CASE WHEN length(CAST(text AS BLOB))<=?2 \
         AND length(CAST(text AS BLOB))<=?3 THEN text ELSE NULL END FROM search_fts WHERE rowid=?1",
            rusqlite::params![row_id, remaining as i64, super::MAX_CHUNK_BYTES as i64],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()
        .map_err(sql_error)?
        .ok_or_else(|| invalid_data("Текст индексированного чанка отсутствует."))?;
    let (bytes, text) = value;
    if !(1..=super::MAX_CHUNK_BYTES as i64).contains(&bytes) {
        return Err(invalid_data("Недопустимый размер индексированного чанка."));
    }
    if bytes as usize > remaining {
        return Ok(None);
    }
    let text = text.ok_or_else(|| invalid_data("Индексированный текст повреждён."))?;
    if text.len() != bytes as usize {
        return Err(invalid_data("Индексированный текст повреждён."));
    }
    Ok(Some(text))
}

pub(super) const REFERENCE_COLUMNS: &str = "c.chunk_id,c.document_id,c.revision_id,c.span_start,c.span_end,c.persona_id,c.grant_id,c.binding_id,c.period_from,c.period_to,c.conversation_id,c.thread_id,c.rowid";
pub(super) fn reference_chunk(
    row: &rusqlite::Row<'_>,
    text: String,
) -> Result<ReferenceChunk, String> {
    Ok(ReferenceChunk {
        chunk_id: row.get(0).map_err(sql_error)?,
        document_id: row.get(1).map_err(sql_error)?,
        revision_id: row.get(2).map_err(sql_error)?,
        span_start: row.get(3).map_err(sql_error)?,
        span_end: row.get(4).map_err(sql_error)?,
        persona_id: row.get(5).map_err(sql_error)?,
        grant_id: row.get(6).map_err(sql_error)?,
        binding_id: row.get(7).map_err(sql_error)?,
        period_from: row.get(8).map_err(sql_error)?,
        period_to: row.get(9).map_err(sql_error)?,
        conversation_id: row.get(10).map_err(sql_error)?,
        thread_id: row.get(11).map_err(sql_error)?,
        text,
    })
}
