//! Bounded exact reads for ranked references. IDs never bypass the caller's
//! authorized scope; an absent/out-of-scope chunk or byte overflow fails the
//! whole read. Results follow requested rank order without changing source text.
use super::{
    ensure_search_schema, invalid_data,
    read::{require_receipt, validate_receipt_identity, with_search_savepoint},
    scan::{bounded_text, reference_chunk, REFERENCE_COLUMNS},
    scope::{build_scope_filter, slot},
    sql_error,
    types::{invalid_id, validate_scope},
    ReferenceChunk, SearchIndexReceipt, SearchScope, MAX_SCAN_LIMIT, MAX_SCAN_TEXT_BYTES,
};
use rusqlite::{Connection, ToSql};
use std::collections::HashSet;
pub fn read_reference_chunks(
    connection: &Connection,
    scope: &SearchScope,
    expected: &SearchIndexReceipt,
    ids: &[String],
    max_text_bytes: usize,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Vec<ReferenceChunk>, String> {
    check()?;
    ensure_search_schema(connection)?;
    validate_scope(scope)?;
    validate_receipt_identity(expected)?;
    let mut seen = HashSet::new();
    if ids.len() > MAX_SCAN_LIMIT
        || !(1..=MAX_SCAN_TEXT_BYTES).contains(&max_text_bytes)
        || ids.iter().any(|id| invalid_id(id) || !seen.insert(id))
    {
        return Err(invalid_data(
            "Неверные ссылки или бюджет чтения фрагментов.",
        ));
    }
    with_search_savepoint(connection, || {
        let receipt = require_receipt(connection, scope, expected)?;
        let mut next = 0;
        let filter = build_scope_filter(scope, receipt.index_generation, &mut next);
        let id_slot = slot(&mut next);
        let sql = format!(
            "SELECT {REFERENCE_COLUMNS} FROM search_chunks c WHERE {} AND c.chunk_id=?{id_slot}",
            filter.sql
        );
        let mut values = filter.params;
        values.push(Box::new(String::new()) as Box<dyn ToSql>);
        let mut statement = connection.prepare(&sql).map_err(sql_error)?;
        let mut result = Vec::with_capacity(ids.len());
        let mut remaining = max_text_bytes;
        for id in ids {
            check()?;
            *values.last_mut().expect("reserved id slot") = Box::new(id.clone());
            let mut rows = statement
                .query(rusqlite::params_from_iter(
                    values.iter().map(|v| v.as_ref()),
                ))
                .map_err(sql_error)?;
            let row = rows
                .next()
                .map_err(sql_error)?
                .ok_or_else(|| invalid_data("Фрагмент отсутствует в разрешённой области."))?;
            let row_id: i64 = row.get(12).map_err(sql_error)?;
            let text = bounded_text(connection, row_id, remaining)?
                .ok_or_else(|| invalid_data("Превышен байтовый бюджет чтения фрагментов."))?;
            remaining -= text.len();
            result.push(reference_chunk(row, text)?);
        }
        check()?;
        Ok(result)
    })
}
