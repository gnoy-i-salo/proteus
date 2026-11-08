//! Read paths for the corpus FTS index: generation-pinned search,
//! receipt-pinned search, exact indexed text, active generation and active
//! publication receipt lookup, and old-generation pruning.
//!
//! Every search runs inside one read snapshot (a SAVEPOINT that nests in a
//! caller transaction): the current `snapshots` row, the active generation
//! pointer, the generation metadata and the hit query are read consistently
//! even while a concurrent rebuild publishes and prunes. A deleted or
//! advanced snapshot never serves its obsolete index. Result pages are
//! bounded and ranked by lower-is-better bm25 with deterministic chunk-id
//! tie-breaks.
//!
//! A `SearchIndexReceipt` is an identity pin, never an access token:
//! `search_with_receipt` compares the full active publication identity
//! (snapshot id/generation, index generation, policy, content digest, chunk
//! count, created time) inside the same pinned read snapshot before the hit
//! query, and `active_receipt` reports the currently active full receipt,
//! staying silent when the snapshot row is missing, unpublished or drifted.
use super::{
    ensure_search_schema, invalid_data,
    query::{fts5_expression, parse_query, ParsedQuery},
    scope::{build_scope_filter, slot},
    sql_error,
    types::{
        invalid_id, validate_scope, SearchHit, SearchIndexReceipt, SearchRequest, SearchResponse,
        SearchScope, MAX_ID_LEN, MAX_JS_SAFE_INTEGER, MAX_LIMIT, MAX_OFFSET, MAX_STAGE_CHUNKS,
    },
};
use rusqlite::{params, Connection, OptionalExtension, ToSql};

/// Read snapshot pinning for one search. A SAVEPOINT joins the connection's
/// current transaction when the caller already holds one and otherwise opens
/// a deferred read transaction: either way every statement of the search
/// reads one WAL snapshot, so a concurrent publish+prune cannot delete the
/// chosen generation between the metadata reads and the hit query.
const SEARCH_SAVEPOINT: &str = "corpus_search_read";

pub fn search(connection: &Connection, request: &SearchRequest) -> Result<SearchResponse, String> {
    ensure_search_schema(connection)?;
    validate_scope(&request.scope)?;
    validate_page(request)?;
    let parsed = parse_query(&request.query_text)?;
    with_search_savepoint(connection, || search_pinned(connection, request, &parsed))
}

/// Receipt-pinned search: the same scope/query/page semantics as [`search`],
/// bound to the exact publication identity the caller expects. The full
/// active receipt is compared before the hit query, inside the same pinned
/// read snapshot; any drift — a rebuild, a policy change, a content/count/
/// created-time mismatch, or a deleted/advanced snapshot — is an explicit
/// error instead of a stale answer. The expected receipt is an identity pin,
/// never an access token: the caller-authorized scope still bounds every
/// hit and grants stay with the owning authority.
pub fn search_with_receipt(
    connection: &Connection,
    request: &SearchRequest,
    expected: &SearchIndexReceipt,
) -> Result<SearchResponse, String> {
    ensure_search_schema(connection)?;
    validate_scope(&request.scope)?;
    validate_page(request)?;
    validate_receipt_identity(expected)?;
    let parsed = parse_query(&request.query_text)?;
    with_search_savepoint(connection, || {
        search_with_receipt_pinned(connection, request, &parsed, expected)
    })
}

/// The currently active full publication receipt for `snapshot_id`. Returns
/// `None` for a missing, unpublished or generation-drifted snapshot: stale
/// or inconsistent metadata never produces an obsolete receipt. This is an
/// identity lookup for receipt-pinned reads, not an authorization decision.
pub fn active_receipt(
    connection: &Connection,
    snapshot_id: &str,
) -> Result<Option<SearchIndexReceipt>, String> {
    ensure_search_schema(connection)?;
    if invalid_id(snapshot_id) {
        return Err(invalid_data("Недопустимый снимок корпуса."));
    }
    load_active_receipt(connection, snapshot_id)
}

/// Runs `body` inside the shared pinned read snapshot: on success the
/// savepoint is released, on error the pre-search state is restored and the
/// savepoint dropped, so a failed search never leaks an open snapshot.
pub(super) fn with_search_savepoint<T>(
    connection: &Connection,
    body: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    connection
        .execute_batch(&format!("SAVEPOINT {SEARCH_SAVEPOINT}"))
        .map_err(sql_error)?;
    match body() {
        Ok(response) => {
            connection
                .execute_batch(&format!("RELEASE SAVEPOINT {SEARCH_SAVEPOINT}"))
                .map_err(sql_error)?;
            Ok(response)
        }
        Err(error) => {
            // Restore the pre-search state and drop the savepoint; the
            // pinned snapshot is released with it.
            let _ = connection.execute_batch(&format!(
                "ROLLBACK TO SAVEPOINT {SEARCH_SAVEPOINT}; RELEASE SAVEPOINT {SEARCH_SAVEPOINT}"
            ));
            Err(error)
        }
    }
}

fn validate_page(request: &SearchRequest) -> Result<(), String> {
    if !(1..=MAX_LIMIT).contains(&request.limit) || !(0..=MAX_OFFSET).contains(&request.offset) {
        return Err(invalid_data("Недопустимые ограничения поиска."));
    }
    Ok(())
}

/// The current `snapshots` row must exist and match the scope: a deleted or
/// advanced snapshot never serves its obsolete index.
pub(super) fn check_current_snapshot(
    connection: &Connection,
    scope: &SearchScope,
) -> Result<(), String> {
    let current_generation: Option<i64> = connection
        .query_row(
            "SELECT generation FROM snapshots WHERE id=?1",
            [&scope.snapshot_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(sql_error)?;
    match current_generation {
        None => Err(invalid_data("Снимок корпуса не найден.")),
        Some(generation) if generation != scope.snapshot_generation => Err(invalid_data(
            "Снимок корпуса изменился; обновите область поиска.",
        )),
        Some(_) => Ok(()),
    }
}

/// One-query lookup of the full active publication receipt. The active
/// pointer, the generation row and the current `snapshots` row must all
/// agree; a missing snapshot row, a dangling pointer or a generation drift
/// means there is no active receipt to report or serve.
pub(super) fn load_active_receipt(
    connection: &Connection,
    snapshot_id: &str,
) -> Result<Option<SearchIndexReceipt>, String> {
    connection
        .query_row(
            "SELECT g.snapshot_id,g.index_generation,g.snapshot_generation,g.policy_fingerprint,\
             g.content_sha256,g.chunk_count,g.created_at,s.generation \
             FROM search_active a \
             JOIN search_generations g ON g.snapshot_id=a.snapshot_id \
             AND g.index_generation=a.index_generation \
             JOIN snapshots s ON s.id=a.snapshot_id \
             WHERE a.snapshot_id=?1",
            [snapshot_id],
            |row| {
                let snapshot_generation = row.get::<_, i64>(2)?;
                let current_generation = row.get::<_, i64>(7)?;
                if current_generation != snapshot_generation {
                    // The snapshot advanced (or its row is inconsistent):
                    // the active publication is obsolete and must not be
                    // reported or served.
                    return Ok(None);
                }
                Ok(Some(SearchIndexReceipt {
                    snapshot_id: row.get(0)?,
                    snapshot_generation,
                    index_generation: row.get(1)?,
                    policy_fingerprint: row.get(3)?,
                    content_sha256: row.get(4)?,
                    chunk_count: row.get(5)?,
                    created_at: row.get(6)?,
                }))
            },
        )
        .optional()
        .map_err(sql_error)
        .map(Option::flatten)
}

/// Bounded structural validation of a caller-supplied expected receipt. The
/// identity comparison below is fail-closed on every field; this only
/// rejects structurally unusable pins early (empty/oversized ids and
/// strings, out-of-range numbers) so no unbounded caller input reaches SQL.
pub(super) fn validate_receipt_identity(receipt: &SearchIndexReceipt) -> Result<(), String> {
    if invalid_id(&receipt.snapshot_id)
        || !(1..=MAX_JS_SAFE_INTEGER).contains(&receipt.snapshot_generation)
        || !(1..=MAX_JS_SAFE_INTEGER).contains(&receipt.index_generation)
        || receipt.policy_fingerprint.is_empty()
        || receipt.policy_fingerprint.len() > MAX_ID_LEN
        || receipt.policy_fingerprint.chars().any(char::is_control)
        || receipt.content_sha256.len() > MAX_ID_LEN
        || receipt.content_sha256.chars().any(char::is_control)
        || !(0..=MAX_STAGE_CHUNKS).contains(&receipt.chunk_count)
        || receipt.created_at.is_empty()
        || receipt.created_at.len() > MAX_ID_LEN
        || receipt.created_at.chars().any(char::is_control)
    {
        return Err(invalid_data("Недопустимая квитанция поискового индекса."));
    }
    Ok(())
}

fn search_pinned(
    connection: &Connection,
    request: &SearchRequest,
    parsed: &ParsedQuery,
) -> Result<SearchResponse, String> {
    check_current_snapshot(connection, &request.scope)?;
    let active = connection
        .query_row(
            "SELECT index_generation FROM search_active WHERE snapshot_id=?1",
            [&request.scope.snapshot_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(sql_error)?
        .ok_or_else(|| invalid_data("Индекс для снимка корпуса не опубликован."))?;
    let snapshot_generation = connection
        .query_row(
            "SELECT snapshot_generation FROM search_generations WHERE snapshot_id=?1 AND index_generation=?2",
            params![request.scope.snapshot_id, active],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(sql_error)?
        .ok_or_else(|| invalid_data("Индекс для снимка корпуса не опубликован."))?;
    if snapshot_generation != request.scope.snapshot_generation {
        return Err(invalid_data(
            "Поколение снимка изменилось; обновите область поиска.",
        ));
    }
    let (hits, has_more) = collect_hits(
        connection,
        &request.scope,
        parsed,
        active,
        request.limit,
        request.offset,
    )?;
    Ok(SearchResponse {
        snapshot_id: request.scope.snapshot_id.clone(),
        snapshot_generation,
        index_generation: active,
        has_more,
        hits,
    })
}

/// Caller holds the shared read savepoint. Every reader pins the full receipt
/// before accessing rows, so a concurrent publication cannot swap the source.
pub(super) fn require_receipt(
    connection: &Connection,
    scope: &SearchScope,
    expected: &SearchIndexReceipt,
) -> Result<SearchIndexReceipt, String> {
    check_current_snapshot(connection, scope)?;
    let actual = load_active_receipt(connection, &scope.snapshot_id)?
        .ok_or_else(|| invalid_data("Индекс для снимка корпуса не опубликован."))?;
    if actual != *expected {
        return Err(invalid_data(
            "Квитанция индекса не соответствует активной публикации; обновите область поиска.",
        ));
    }
    Ok(actual)
}

fn search_with_receipt_pinned(
    connection: &Connection,
    request: &SearchRequest,
    parsed: &ParsedQuery,
    expected: &SearchIndexReceipt,
) -> Result<SearchResponse, String> {
    let actual = require_receipt(connection, &request.scope, expected)?;
    let (hits, has_more) = collect_hits(
        connection,
        &request.scope,
        parsed,
        actual.index_generation,
        request.limit,
        request.offset,
    )?;
    Ok(SearchResponse {
        snapshot_id: actual.snapshot_id.clone(),
        snapshot_generation: actual.snapshot_generation,
        index_generation: actual.index_generation,
        has_more,
        hits,
    })
}

fn collect_hits(
    connection: &Connection,
    scope: &SearchScope,
    parsed: &ParsedQuery,
    index_generation: i64,
    limit: i64,
    offset: i64,
) -> Result<(Vec<SearchHit>, bool), String> {
    let query = build_search_sql(scope, parsed, index_generation, limit + 1, offset);
    let mut statement = connection.prepare(&query.sql).map_err(sql_error)?;
    let mut rows = statement
        .query(rusqlite::params_from_iter(
            query.params.iter().map(|value| value.as_ref()),
        ))
        .map_err(sql_error)?;
    let mut hits = Vec::new();
    let mut has_more = false;
    while let Some(row) = rows.next().map_err(sql_error)? {
        if hits.len() as i64 == limit {
            has_more = true;
            break;
        }
        hits.push(SearchHit {
            chunk_id: row.get(0).map_err(sql_error)?,
            snapshot_id: row.get(1).map_err(sql_error)?,
            index_generation: row.get(2).map_err(sql_error)?,
            bm25: row.get(3).map_err(sql_error)?,
            document_id: row.get(4).map_err(sql_error)?,
            revision_id: row.get(5).map_err(sql_error)?,
            span_start: row.get(6).map_err(sql_error)?,
            span_end: row.get(7).map_err(sql_error)?,
            persona_id: row.get(8).map_err(sql_error)?,
            grant_id: row.get(9).map_err(sql_error)?,
            binding_id: row.get(10).map_err(sql_error)?,
            period_from: row.get(11).map_err(sql_error)?,
            period_to: row.get(12).map_err(sql_error)?,
            conversation_id: row.get(13).map_err(sql_error)?,
            thread_id: row.get(14).map_err(sql_error)?,
        });
    }
    Ok((hits, has_more))
}

struct ScopedQuery {
    sql: String,
    params: Vec<Box<dyn ToSql>>,
}
fn build_search_sql(
    scope: &SearchScope,
    parsed: &ParsedQuery,
    index_generation: i64,
    limit: i64,
    offset: i64,
) -> ScopedQuery {
    let mut next = 0i64;
    let match_param = slot(&mut next);
    let filter = build_scope_filter(scope, index_generation, &mut next);
    let limit_param = slot(&mut next);
    let offset_param = slot(&mut next);
    let filter_sql = filter.sql;
    let sql = format!(
        "SELECT c.chunk_id,c.snapshot_id,c.index_generation,bm25(search_fts) AS score,c.document_id,\
         c.revision_id,c.span_start,c.span_end,c.persona_id,c.grant_id,c.binding_id,\
         c.period_from,c.period_to,c.conversation_id,c.thread_id \
         FROM search_fts JOIN search_chunks c ON c.rowid=search_fts.rowid \
         WHERE search_fts MATCH ?{match_param} AND {filter_sql} \
         ORDER BY score ASC,c.chunk_id ASC LIMIT ?{limit_param} OFFSET ?{offset_param}"
    );
    let mut values: Vec<Box<dyn ToSql>> = vec![Box::new(fts5_expression(parsed))];
    values.extend(filter.params);
    values.push(Box::new(limit));
    values.push(Box::new(offset));
    ScopedQuery {
        sql,
        params: values,
    }
}

pub fn indexed_text(
    connection: &Connection,
    snapshot_id: &str,
    index_generation: i64,
    chunk_id: &str,
) -> Result<Option<String>, String> {
    ensure_search_schema(connection)?;
    if invalid_id(snapshot_id) || invalid_id(chunk_id) || index_generation <= 0 {
        return Err(invalid_data("Недопустимая ссылка на индексированный чанк."));
    }
    connection
        .query_row(
            "SELECT f.text FROM search_fts f JOIN search_chunks c ON c.rowid=f.rowid \
             WHERE c.snapshot_id=?1 AND c.index_generation=?2 AND c.chunk_id=?3",
            params![snapshot_id, index_generation, chunk_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)
}

pub fn active_generation(
    connection: &Connection,
    snapshot_id: &str,
) -> Result<Option<(i64, i64)>, String> {
    ensure_search_schema(connection)?;
    if invalid_id(snapshot_id) {
        return Err(invalid_data("Недопустимый снимок корпуса."));
    }
    connection
        .query_row(
            "SELECT g.index_generation,g.snapshot_generation FROM search_generations g \
             JOIN search_active a ON a.snapshot_id=g.snapshot_id \
             AND a.index_generation=g.index_generation WHERE g.snapshot_id=?1",
            [snapshot_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sql_error)
}

pub fn prune_generations(connection: &mut Connection, snapshot_id: &str) -> Result<(), String> {
    ensure_search_schema(connection)?;
    if invalid_id(snapshot_id) {
        return Err(invalid_data("Недопустимый снимок корпуса."));
    }
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let active: Option<i64> = tx
        .query_row(
            "SELECT index_generation FROM search_active WHERE snapshot_id=?1",
            [&snapshot_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)?;
    tx.execute(
        "DELETE FROM search_fts WHERE rowid IN (SELECT rowid FROM search_chunks \
         WHERE snapshot_id=?1 AND index_generation IS NOT ?2)",
        params![snapshot_id, active],
    )
    .map_err(sql_error)?;
    tx.execute(
        "DELETE FROM search_chunks WHERE snapshot_id=?1 AND index_generation IS NOT ?2",
        params![snapshot_id, active],
    )
    .map_err(sql_error)?;
    tx.execute(
        "DELETE FROM search_generations WHERE snapshot_id=?1 AND index_generation IS NOT ?2",
        params![snapshot_id, active],
    )
    .map_err(sql_error)?;
    tx.commit().map_err(sql_error)
}
