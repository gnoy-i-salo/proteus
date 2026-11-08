//! Shared scope-filter SQL and parameters for authorized corpus index reads.
//!
//! The FTS hit query (`read::collect_hits`) and the reference-chunk scan
//! (`scan`) must restrict exactly the same rows: one snapshot, one index
//! generation, one persona, the same grant/binding membership lists and the
//! same dated/undated period semantics. This builder is the single source of
//! that filter; both readers compose their own projection, ordering and
//! pagination around it. The clause shape, parameter order and `?N` slot
//! numbering are observable behavior characterized by the existing FTS
//! regression tests.
use super::types::SearchScope;
use rusqlite::ToSql;

pub(in crate::corpus_database) struct ScopedFilter {
    pub sql: String,
    pub params: Vec<Box<dyn ToSql>>,
}

pub(in crate::corpus_database) fn slot(next: &mut i64) -> String {
    *next += 1;
    next.to_string()
}

/// Builds `c.snapshot_id=? AND c.index_generation=? AND c.persona_id=? AND
/// c.grant_id IN (...) AND c.binding_id IN (...)` plus the optional period
/// clause, with parameters in the historical order (time filter: undated
/// flag, upper bound, then lower bound). `next` continues the caller's `?N`
/// parameter numbering so composed statements keep stable slots.
pub(in crate::corpus_database) fn build_scope_filter(
    scope: &SearchScope,
    index_generation: i64,
    next: &mut i64,
) -> ScopedFilter {
    let snapshot_param = slot(next);
    let generation_param = slot(next);
    let persona_param = slot(next);
    let grant_clause = (0..scope.grant_ids.len())
        .map(|_| format!("?{}", slot(next)))
        .collect::<Vec<_>>()
        .join(",");
    let binding_clause = (0..scope.binding_ids.len())
        .map(|_| format!("?{}", slot(next)))
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
            let undated_flag = slot(next);
            let to_param = slot(next);
            let from_param = slot(next);
            format!(
                " AND (((c.period_from IS NULL AND c.period_to IS NULL) AND ?{undated_flag}) OR \
                 ((c.period_from IS NOT NULL OR c.period_to IS NOT NULL) AND \
                  (c.period_from IS NULL OR ?{to_param} IS NULL OR c.period_from <= ?{to_param}) AND \
                  (c.period_to IS NULL OR ?{from_param} IS NULL OR c.period_to >= ?{from_param})))"
            )
        }
    };
    let sql = format!(
        "c.snapshot_id=?{snapshot_param} AND c.index_generation=?{generation_param} \
         AND c.persona_id=?{persona_param} AND c.grant_id IN ({grant_clause}) \
         AND c.binding_id IN ({binding_clause}){time_clause}"
    );
    let mut params: Vec<Box<dyn ToSql>> = vec![
        Box::new(scope.snapshot_id.clone()),
        Box::new(index_generation),
        Box::new(scope.persona_id.clone()),
    ];
    params.extend(
        scope
            .grant_ids
            .iter()
            .cloned()
            .map(|id| Box::new(id) as Box<dyn ToSql>),
    );
    params.extend(
        scope
            .binding_ids
            .iter()
            .cloned()
            .map(|id| Box::new(id) as Box<dyn ToSql>),
    );
    if scope.period.from.is_some() || scope.period.to.is_some() {
        params.push(Box::new(scope.period.include_undated as i64));
        params.push(Box::new(scope.period.to));
        params.push(Box::new(scope.period.from));
    }
    ScopedFilter { sql, params }
}
