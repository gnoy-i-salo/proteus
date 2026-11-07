//! Read paths for the corpus FTS index: generation-pinned search, exact
//! indexed text, active generation lookup, and old-generation pruning.
//!
//! Every search runs inside one read snapshot (a SAVEPOINT that nests in a
//! caller transaction): the current `snapshots` row, the active generation
//! pointer, the generation metadata and the hit query are read consistently
//! even while a concurrent rebuild publishes and prunes. A deleted or
//! advanced snapshot never serves its obsolete index. Result pages are
//! bounded and ranked by lower-is-better bm25 with deterministic chunk-id
//! tie-breaks.
use super::{
    ensure_search_schema, invalid_data,
    query::{fts5_expression, parse_query, ParsedQuery},
    sql_error,
    types::{invalid_id, validate_scope, SearchHit, SearchRequest, SearchResponse, SearchScope},
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
    if !(1..=super::types::MAX_LIMIT).contains(&request.limit)
        || !(0..=super::types::MAX_OFFSET).contains(&request.offset)
    {
        return Err(invalid_data("Недопустимые ограничения поиска."));
    }
    let parsed = parse_query(&request.query_text)?;
    connection
        .execute_batch(&format!("SAVEPOINT {SEARCH_SAVEPOINT}"))
        .map_err(sql_error)?;
    let result = search_pinned(connection, request, &parsed);
    match result {
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
fn search_pinned(
    connection: &Connection,
    request: &SearchRequest,
    parsed: &ParsedQuery,
) -> Result<SearchResponse, String> {
    // The current snapshot row must exist and match the scope: a deleted or
    // advanced snapshot never serves its obsolete index.
    let current_generation: Option<i64> = connection
        .query_row(
            "SELECT generation FROM snapshots WHERE id=?1",
            [&request.scope.snapshot_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(sql_error)?;
    match current_generation {
        None => return Err(invalid_data("Снимок корпуса не найден.")),
        Some(generation) if generation != request.scope.snapshot_generation => {
            return Err(invalid_data(
                "Снимок корпуса изменился; обновите область поиска.",
            ));
        }
        Some(_) => {}
    }
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
    let query = build_search_sql(
        &request.scope,
        parsed,
        active,
        request.limit + 1,
        request.offset,
    );
    let mut statement = connection.prepare(&query.sql).map_err(sql_error)?;
    let mut rows = statement
        .query(rusqlite::params_from_iter(
            query.params.iter().map(|value| value.as_ref()),
        ))
        .map_err(sql_error)?;
    let mut hits = Vec::new();
    let mut has_more = false;
    while let Some(row) = rows.next().map_err(sql_error)? {
        if hits.len() as i64 == request.limit {
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
    Ok(SearchResponse {
        snapshot_id: request.scope.snapshot_id.clone(),
        snapshot_generation,
        index_generation: active,
        has_more,
        hits,
    })
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
    let slot = |next: &mut i64| {
        *next += 1;
        next.to_string()
    };
    let match_param = slot(&mut next);
    let snapshot_param = slot(&mut next);
    let generation_param = slot(&mut next);
    let persona_param = slot(&mut next);
    let grant_clause = (0..scope.grant_ids.len())
        .map(|_| format!("?{}", slot(&mut next)))
        .collect::<Vec<_>>()
        .join(",");
    let binding_clause = (0..scope.binding_ids.len())
        .map(|_| format!("?{}", slot(&mut next)))
        .collect::<Vec<_>>()
        .join(",");
    let time_clause = match (
        scope.period.from,
        scope.period.to,
        scope.period.include_undated,
    ) {
        (None, None, true) => String::new(),
        (None, None, false) => " AND (c.period_from IS NOT NULL OR c.period_to IS NOT NULL)".into(),
        (Some(_), _, _) | (_, Some(_), _) => {
            let undated_flag = slot(&mut next);
            let to_param = slot(&mut next);
            let from_param = slot(&mut next);
            format!(
                " AND (((c.period_from IS NULL AND c.period_to IS NULL) AND ?{undated_flag}) OR \
                 ((c.period_from IS NOT NULL OR c.period_to IS NOT NULL) AND \
                  (c.period_from IS NULL OR ?{to_param} IS NULL OR c.period_from <= ?{to_param}) AND \
                  (c.period_to IS NULL OR ?{from_param} IS NULL OR c.period_to >= ?{from_param})))"
            )
        }
    };
    let limit_param = slot(&mut next);
    let offset_param = slot(&mut next);
    let sql = format!(
        "SELECT c.chunk_id,c.snapshot_id,c.index_generation,bm25(search_fts) AS score,c.document_id,\
         c.revision_id,c.span_start,c.span_end,c.persona_id,c.grant_id,c.binding_id,\
         c.period_from,c.period_to,c.conversation_id,c.thread_id \
         FROM search_fts JOIN search_chunks c ON c.rowid=search_fts.rowid \
         WHERE search_fts MATCH ?{match_param} AND c.snapshot_id=?{snapshot_param} \
         AND c.index_generation=?{generation_param} AND c.persona_id=?{persona_param} \
         AND c.grant_id IN ({grant_clause}) AND c.binding_id IN ({binding_clause}){time_clause} \
         ORDER BY score ASC,c.chunk_id ASC LIMIT ?{limit_param} OFFSET ?{offset_param}"
    );
    let mut values: Vec<Box<dyn ToSql>> = vec![
        Box::new(fts5_expression(parsed)),
        Box::new(scope.snapshot_id.clone()),
        Box::new(index_generation),
        Box::new(scope.persona_id.clone()),
    ];
    values.extend(
        scope
            .grant_ids
            .iter()
            .cloned()
            .map(|id| Box::new(id) as Box<dyn ToSql>),
    );
    values.extend(
        scope
            .binding_ids
            .iter()
            .cloned()
            .map(|id| Box::new(id) as Box<dyn ToSql>),
    );
    if scope.period.from.is_some() || scope.period.to.is_some() {
        values.push(Box::new(scope.period.include_undated as i64));
        values.push(Box::new(scope.period.to));
        values.push(Box::new(scope.period.from));
    }
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
