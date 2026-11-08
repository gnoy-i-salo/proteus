//! Host-bound retrieval state. Scope and limits are chosen once by native
//! orchestration; model actions can only select a query or an episode ID.
use super::budget::{RecallLedger, RecallLimits, RetrievalCharge};
use crate::{
    corpus_database::search::{SearchIndexReceipt, SearchPeriod},
    ingestion::{
        episode_read::{self, EpisodeReadInput, EpisodeReadResult},
        grants::GrantUse,
        index_authority::IndexAuthority,
        operations::Operation,
    },
    sources::Sources,
};
pub struct SessionInput<'a> {
    pub job_id: &'a str,
    pub snapshot_id: &'a str,
    pub persona_id: &'a str,
    pub period: SearchPeriod,
    pub limits: RecallLimits,
}
pub struct MemorySession {
    sources: Sources,
    operation: Operation,
    authority: IndexAuthority,
    source: SearchIndexReceipt,
    persona_id: String,
    period: SearchPeriod,
    ledger: RecallLedger,
}
impl MemorySession {
    pub fn bind(
        sources: Sources,
        operation: Operation,
        input: SessionInput<'_>,
        now_ms: impl Fn() -> i64,
    ) -> Result<Self, String> {
        let started = now_ms();
        let ledger = RecallLedger::new(input.limits, started)?;
        operation.check()?;
        let (authority, source) = sources.with_ingestion_database(&operation, |db| {
            let authority = IndexAuthority::resolve(
                db,
                &operation,
                input.job_id,
                Some(input.persona_id),
                started,
            )?;
            authority.current(db, started, GrantUse::Model)?;
            let check = || {
                let now = now_ms();
                operation.check()?;
                if now < started || now >= authority.deadline {
                    return Err("Разрешение источника истекло.".into());
                }
                Ok(())
            };
            let pin = authority.pin_index(db, input.snapshot_id, &check)?;
            let scope = authority.scope(&pin, input.persona_id, input.period.clone())?;
            db.read_reference_chunks(&scope, &pin.receipt, &[], 1, &check)?;
            check()?;
            Ok((authority, pin.receipt))
        })?;
        Ok(Self {
            sources,
            operation,
            authority,
            source,
            persona_id: input.persona_id.into(),
            period: input.period,
            ledger,
        })
    }
    pub fn read_episode(
        &mut self,
        episode_id: &str,
        now_ms: impl Fn() -> i64,
    ) -> Result<EpisodeReadResult, String> {
        let Self {
            sources,
            operation,
            authority,
            source,
            persona_id,
            period,
            ledger,
        } = self;
        let guard = || {
            sources.with_ingestion_database(operation, |db| {
                let now = now_ms();
                operation.check()?;
                authority.current(db, now, GrantUse::Model)?;
                if db.active_search_receipt(&source.snapshot_id)?.as_ref() != Some(source) {
                    return Err("Источник или индекс изменился.".into());
                }
                let finished = now_ms();
                operation.check()?;
                if finished < now || finished >= authority.deadline {
                    return Err("Разрешение источника истекло.".into());
                }
                Ok(finished)
            })
        };
        ledger.run_checked(guard, |allowance| {
            let result = episode_read::read(
                sources,
                operation,
                &EpisodeReadInput {
                    job_id: &authority.job.id,
                    snapshot_id: &source.snapshot_id,
                    persona_id,
                    episode_id,
                    period: period.clone(),
                    expected_index_generation: Some(source.index_generation),
                    max_chunks: allowance.remaining.candidates.min(100),
                    max_source_bytes: allowance.remaining.source_bytes,
                    // Separate cap for stored metadata/original pages, not process RSS.
                    max_stored_bytes: 16 * 1024 * 1024,
                    max_context_tokens: allowance.remaining.context_tokens,
                    max_duration_ms: allowance.duration_ms.min(60000),
                },
                &now_ms,
            )?;
            if result.source != *source {
                return Err("Источник или индекс изменился.".into());
            }
            let charge = RetrievalCharge {
                candidates: result.entries.len(),
                episodes: 1,
                source_bytes: result.source_bytes,
                context_tokens: result.context_tokens,
            };
            Ok((result, charge))
        })
    }
    pub fn usage(&self) -> (usize, RetrievalCharge) {
        self.ledger.usage()
    }
}
