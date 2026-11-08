//! Semantic candidates over the same authorized reference scope as lexical search.
use super::{read, sql_error, VectorReceipt, INVALID};
use crate::{
    corpus_database::search::{self, SearchScope},
    local_embedding::search::{search_stream, SearchBudget, SemanticHit, VectorCandidate},
};
use rusqlite::{Connection, ToSql};

pub fn search_vectors(
    connection: &mut Connection,
    scope: &SearchScope,
    expected: &VectorReceipt,
    query: &[f32],
    fingerprint: &str,
    budget: SearchBudget,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Vec<SemanticHit>, String> {
    check()?;
    search::validate_scope(scope)?;
    let tx = connection.savepoint().map_err(sql_error)?;
    let actual = read::load_active(&tx, &scope.snapshot_id)?.ok_or_else(|| INVALID.to_string())?;
    if actual != *expected
        || scope.snapshot_generation != actual.source.snapshot_generation
        || fingerprint != actual.model_fingerprint
        || query.len() != actual.dimensions
    {
        return Err(INVALID.into());
    }
    let hits = search_stream(query, fingerprint, budget, &|| check(), |accept| {
        let mut next = 0;
        let mut filter =
            search::scope::build_scope_filter(scope, actual.source.index_generation, &mut next);
        let generation_slot = search::scope::slot(&mut next);
        let limit_slot = search::scope::slot(&mut next);
        let sql = format!(
            "SELECT c.chunk_id,c.document_id,e.vector FROM vector_entries e \
             JOIN search_chunks c ON c.chunk_id=e.chunk_id WHERE {} \
             AND e.generation=?{generation_slot} LIMIT ?{limit_slot}",
            filter.sql
        );
        filter
            .params
            .push(Box::new(actual.generation) as Box<dyn ToSql>);
        filter
            .params
            .push(Box::new((budget.max_scanned + 1) as i64));
        let mut statement = tx.prepare(&sql).map_err(sql_error)?;
        let mut rows = statement
            .query(rusqlite::params_from_iter(
                filter.params.iter().map(|value| value.as_ref()),
            ))
            .map_err(sql_error)?;
        loop {
            check()?;
            let Some(row) = rows.next().map_err(sql_error)? else {
                break;
            };
            let chunk_id: String = row.get(0).map_err(sql_error)?;
            let document_id: String = row.get(1).map_err(sql_error)?;
            let bytes: Vec<u8> = row.get(2).map_err(sql_error)?;
            if bytes.len() != actual.dimensions * 4 {
                return Err(INVALID.into());
            }
            let vector: Vec<f32> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| f32::from_le_bytes(*chunk))
                .collect();
            accept(VectorCandidate {
                chunk_id: &chunk_id,
                document_id: &document_id,
                fingerprint,
                vector: &vector,
            })?;
        }
        Ok(())
    })?;
    check()?;
    tx.commit().map_err(sql_error)?;
    Ok(hits)
}
