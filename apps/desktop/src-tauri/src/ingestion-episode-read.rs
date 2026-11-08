//! Native bounded episode expansion. Host supplies the job/persona/snapshot;
//! a model-proposed episode ID never widens the persisted reference scope.
use super::{grants::GrantUse, index_authority::IndexAuthority, operations::Operation};
use crate::{
    corpus_database::{
        episodes::PreparedEpisode,
        preparation::PreparedEntry,
        search::{SearchIndexReceipt, SearchPeriod, MAX_JS_SAFE_INTEGER},
    },
    sources::Sources,
};
use std::sync::Arc;
pub struct EpisodeReadInput<'a> {
    pub job_id: &'a str,
    pub snapshot_id: &'a str,
    pub persona_id: &'a str,
    pub episode_id: &'a str,
    pub period: SearchPeriod,
    pub expected_index_generation: Option<i64>,
    pub max_chunks: usize,
    pub max_source_bytes: usize,
    pub max_stored_bytes: usize,
    pub max_context_tokens: usize,
    pub max_duration_ms: i64,
}
pub struct EpisodeReadResult {
    pub source: SearchIndexReceipt,
    pub episode: Arc<PreparedEpisode>,
    pub entries: Vec<PreparedEntry>,
    pub source_bytes: usize,
    pub context_tokens: usize,
}
impl EpisodeReadInput<'_> {
    fn validate(&self) -> Result<(), String> {
        if [
            self.job_id,
            self.snapshot_id,
            self.persona_id,
            self.episode_id,
        ]
        .iter()
        .any(|v| v.trim().is_empty() || v.len() > 256 || v.chars().any(char::is_control))
            || !(1..=100).contains(&self.max_chunks)
            || !(1..=64 * 1024 * 1024).contains(&self.max_source_bytes)
            || !(1..=64 * 1024 * 1024).contains(&self.max_stored_bytes)
            || !(1..=1000000).contains(&self.max_context_tokens)
            || !(1..=60000).contains(&self.max_duration_ms)
            || self
                .expected_index_generation
                .is_some_and(|v| !(1..=MAX_JS_SAFE_INTEGER).contains(&v))
            || self
                .period
                .from
                .zip(self.period.to)
                .is_some_and(|(a, b)| a > b)
            || [self.period.from, self.period.to]
                .into_iter()
                .flatten()
                .any(|v| !(-MAX_JS_SAFE_INTEGER..=MAX_JS_SAFE_INTEGER).contains(&v))
        {
            return Err("Неверная область или бюджет чтения эпизода.".into());
        }
        Ok(())
    }
}
pub fn read(
    sources: &Sources,
    operation: &Operation,
    input: &EpisodeReadInput<'_>,
    now_ms: impl Fn() -> i64,
) -> Result<EpisodeReadResult, String> {
    input.validate()?;
    let started = now_ms();
    let end = started
        .checked_add(input.max_duration_ms)
        .filter(|_| started >= 0)
        .ok_or("Неверное время чтения.")?;
    let budget = || {
        let now = now_ms();
        operation.check()?;
        if now < started || now >= end {
            return Err("Превышено время чтения эпизода.".to_string());
        }
        Ok(now)
    };
    sources.with_ingestion_database(operation, |db| {
        let authority = IndexAuthority::resolve(
            db,
            operation,
            input.job_id,
            Some(input.persona_id),
            budget()?,
        )?;
        authority.current(db, budget()?, GrantUse::Model)?;
        let check = || {
            if budget()? >= authority.deadline {
                return Err("Разрешение источника истекло.".into());
            }
            Ok(())
        };
        let pin = authority.pin_index(db, input.snapshot_id, &check)?;
        if input
            .expected_index_generation
            .is_some_and(|v| v != pin.receipt.index_generation)
        {
            return Err("Версия индекса изменилась.".into());
        }
        let cursor = db.prepared_cursor(input.snapshot_id, &check)?;
        let references = db.prepared_episode_references(
            &cursor,
            input.episode_id,
            input.max_chunks,
            input.max_stored_bytes,
            &check,
        )?;
        let binding = authority
            .bindings
            .iter()
            .find(|b| b.id == references.episode.binding_id && b.persona_id == input.persona_id)
            .ok_or("Эпизод вне разрешённой области.")?;
        let scope = authority.scope(&pin, input.persona_id, input.period.clone())?;
        let chunks = db.read_reference_chunks(
            &scope,
            &pin.receipt,
            &references.chunk_ids,
            input.max_source_bytes,
            &check,
        )?;
        let entries = db.prepared_entries(
            &cursor,
            &references.chunk_ids,
            input.max_stored_bytes,
            &check,
        )?;
        if chunks.len() != entries.len() {
            return Err("Неполный эпизод.".into());
        }
        let mut source_bytes = 0usize;
        let mut context_tokens = 0usize;
        for (chunk, entry) in chunks.iter().zip(&entries) {
            check()?;
            if entry.episode.id != input.episode_id || entry.episode.binding_id != binding.id {
                return Err("Эпизод не соответствует проверенным оригиналам.".into());
            }
            authority.verify_reference_entry(chunk, entry, input.persona_id, &check)?;
            source_bytes = source_bytes
                .checked_add(entry.chunk.text.len())
                .ok_or("Превышен бюджет чтения.")?;
            context_tokens = context_tokens
                .checked_add(entry.chunk.token_count)
                .ok_or("Превышен бюджет токенов.")?;
            if source_bytes > input.max_source_bytes || context_tokens > input.max_context_tokens {
                return Err("Превышен бюджет чтения эпизода.".into());
            }
        }
        authority.current(db, budget()?, GrantUse::Model)?;
        if db.active_search_receipt(input.snapshot_id)?.as_ref() != Some(&pin.receipt) {
            return Err("Источник или индекс изменился.".into());
        }
        check()?;
        Ok(EpisodeReadResult {
            source: pin.receipt,
            episode: references.episode,
            entries,
            source_bytes,
            context_tokens,
        })
    })
}
