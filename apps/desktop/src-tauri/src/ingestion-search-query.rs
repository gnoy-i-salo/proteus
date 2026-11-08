//! Authorized local lexical queries. Caller chooses a persona and narrows time;
//! grant/binding pairs always come from the persisted native job. Corpus access
//! here does not authorize sending results to a model or exporting originals.
use super::{grants::GrantUse, index_authority::IndexAuthority, operations::Operation};
use crate::{
    corpus_database::search::{
        SearchPeriod, SearchRequest, SearchResponse, MAX_JS_SAFE_INTEGER, MAX_LIMIT, MAX_OFFSET,
        MAX_QUERY_BYTES,
    },
    sources::Sources,
};
pub struct QueryInput<'a> {
    pub job_id: &'a str,
    pub snapshot_id: &'a str,
    pub persona_id: &'a str,
    pub query_text: &'a str,
    pub period: SearchPeriod,
    pub limit: i64,
    pub offset: i64,
    pub expected_index_generation: Option<i64>,
    pub max_duration_ms: i64,
}
impl QueryInput<'_> {
    pub(super) fn validate(&self) -> Result<(), String> {
        if [self.job_id, self.snapshot_id, self.persona_id]
            .iter()
            .any(|id| id.trim().is_empty() || id.len() > 256 || id.chars().any(char::is_control))
            || self.query_text.is_empty()
            || self.query_text.len() > MAX_QUERY_BYTES
            || !(1..=MAX_LIMIT).contains(&self.limit)
            || !(0..=MAX_OFFSET).contains(&self.offset)
            || !(1..=60000).contains(&self.max_duration_ms)
            || self
                .expected_index_generation
                .is_some_and(|v| !(1..=MAX_JS_SAFE_INTEGER).contains(&v))
            || (self.offset > 0 && self.expected_index_generation.is_none())
            || self
                .period
                .from
                .zip(self.period.to)
                .is_some_and(|(from, to)| from > to)
            || [self.period.from, self.period.to]
                .into_iter()
                .flatten()
                .any(|v| !(-MAX_JS_SAFE_INTEGER..=MAX_JS_SAFE_INTEGER).contains(&v))
        {
            return Err("Неверные параметры поиска корпуса.".into());
        }
        Ok(())
    }
}
pub fn query(
    sources: &Sources,
    operation: &Operation,
    input: &QueryInput<'_>,
    now_ms: impl Fn() -> i64,
) -> Result<SearchResponse, String> {
    input.validate()?;
    let started = now_ms();
    let end = started
        .checked_add(input.max_duration_ms)
        .filter(|_| started >= 0)
        .ok_or("Неверное время поиска.")?;
    let budget = || -> Result<i64, String> {
        let now = now_ms();
        operation.check()?;
        if now < started || now >= end {
            return Err("Превышено время поиска.".into());
        }
        Ok(now)
    };
    budget()?;
    sources.with_ingestion_database(operation, |db| {
        budget()?;
        let authority = IndexAuthority::resolve(
            db,
            operation,
            input.job_id,
            Some(input.persona_id),
            now_ms(),
        )?;
        let check = || -> Result<(), String> {
            let current = budget()?;
            if current >= authority.deadline {
                return Err("Разрешение источника истекло во время поиска.".into());
            }
            Ok(())
        };
        let pin = authority.pin_index(db, input.snapshot_id, &check)?;
        let receipt = &pin.receipt;
        if input
            .expected_index_generation
            .is_some_and(|v| v != receipt.index_generation)
        {
            return Err("Версия индекса изменилась; начните поиск заново.".into());
        }
        check()?;
        let request = SearchRequest {
            scope: authority.scope(&pin, input.persona_id, input.period.clone())?,
            query_text: input.query_text.into(),
            limit: input.limit,
            offset: input.offset,
        };
        let response = db.search_corpus_with_receipt(&request, receipt)?;
        if response.hits.iter().any(|hit| {
            !authority.bindings.iter().any(|b| {
                hit.persona_id == b.persona_id
                    && hit.binding_id == b.id
                    && hit.grant_id == b.grant_id
            })
        }) {
            return Err("Результат не принадлежит разрешённой привязке.".into());
        }
        // A stored receipt is not authority. Recheck before any result escapes.
        authority.current(db, now_ms(), GrantUse::Corpus)?;
        check()?;
        // The search savepoint is already released. Observe the current active
        // receipt last: deleting a source cascades its snapshot even when the
        // separate grant and completed job are still valid.
        if db.active_search_receipt(input.snapshot_id)?.as_ref() != Some(receipt) {
            return Err("Индекс или источник изменился во время поиска.".into());
        }
        // Final metadata I/O also consumes time and can overlap cancellation.
        check()?;
        Ok(response)
    })
}
