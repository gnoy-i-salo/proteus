//! Host-bound retrieval state. Scope and limits are chosen once by native
//! orchestration; model actions can only select a query or an episode ID.
#[path = "memory-model-session.rs"]
mod model_session;
#[path = "memory-recall.rs"]
mod recall;
#[path = "memory-receipts.rs"]
mod receipts;
use super::budget::{RecallLedger, RecallLimits, RetrievalCharge};
pub use super::context_input::PreparedModelInput;
pub use super::model_contexts::{
    CompletedModelContext, DispatchInput, InvocationBinding, ReservedContext,
};
pub use super::research_support::VerifiedResearchSupport;
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
pub use recall::{NativeRecallHit, NativeRecallResult};
pub use receipts::{RecordedRetrieval, RetrievalReceipt};
pub struct SessionInput<'a> {
    pub job_id: &'a str,
    pub snapshot_id: &'a str,
    pub persona_id: &'a str,
    pub period: SearchPeriod,
    pub limits: RecallLimits,
}
pub struct MemorySession {
    scope: MemoryScope,
    ledger: RecallLedger,
    history: super::receipt_history::ReceiptHistory,
    models: Option<super::model_contexts::ModelContexts>,
}
struct MemoryScope {
    deadline: i64,
    sources: Sources,
    operation: Operation,
    authority: IndexAuthority,
    source: SearchIndexReceipt,
    persona_id: String,
    period: SearchPeriod,
    hybrid: Option<(
        crate::corpus_database::vectors::VectorReceipt,
        crate::local_embedding::runtime::EmbeddingBudget,
    )>,
}
impl MemorySession {
    pub(crate) fn cancel(&self) {
        self.cancellation_handle()
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    pub(super) fn cancellation_handle(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.scope.operation.cancellation_handle()
    }
    pub(super) fn verify_current(&mut self, now_ms: impl Fn() -> i64) -> Result<(), String> {
        self.ledger.verify(|| self.scope.guard(&now_ms))?;
        // The terminal clock check must include both authorities. A later
        // source-only check cannot return success after model authority expires.
        if let Some(models) = &mut self.models {
            models.verify(&|| self.scope.guard(&now_ms))?;
        }
        Ok(())
    }
    pub fn bind(
        sources: Sources,
        operation: Operation,
        input: SessionInput<'_>,
        now_ms: impl Fn() -> i64,
    ) -> Result<Self, String> {
        let started = now_ms();
        let ledger = RecallLedger::new(input.limits, started)?;
        // The ledger has already checked this addition for overflow.
        let deadline = started + input.limits.max_duration_ms;
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
                if now < started || now >= authority.deadline || now >= deadline {
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
            scope: MemoryScope {
                deadline: deadline.min(authority.deadline),
                sources,
                operation,
                authority,
                source,
                persona_id: input.persona_id.into(),
                period: input.period,
                hybrid: None,
            },
            ledger,
            history: Default::default(),
            models: None,
        })
    }
    pub fn read_episode(
        &mut self,
        episode_id: &str,
        now_ms: impl Fn() -> i64,
    ) -> Result<EpisodeReadResult, String> {
        let MemoryScope {
            sources,
            operation,
            authority,
            source,
            persona_id,
            period,
            ..
        } = &self.scope;
        self.ledger.run_checked(
            || self.scope.guard(&now_ms),
            |allowance| {
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
            },
        )
    }
    pub fn usage(&self) -> (usize, RetrievalCharge) {
        self.ledger.usage()
    }
}
impl MemoryScope {
    fn guard(&self, now_ms: &dyn Fn() -> i64) -> Result<i64, String> {
        self.sources.with_ingestion_database(&self.operation, |db| {
            let now = now_ms();
            self.operation.check()?;
            self.authority.current(db, now, GrantUse::Model)?;
            if db.active_search_receipt(&self.source.snapshot_id)?.as_ref() != Some(&self.source) {
                return Err("Источник или индекс изменился.".into());
            }
            if let Some((vectors, _)) = &self.hybrid {
                if db
                    .active_vector_receipt(&self.source.snapshot_id, &|| self.operation.check())?
                    .as_ref()
                    != Some(vectors)
                {
                    return Err("Векторный индекс сессии изменился.".into());
                }
            }
            let finished = now_ms();
            self.operation.check()?;
            if finished < now || finished >= self.deadline {
                return Err("Разрешение источника истекло.".into());
            }
            Ok(finished)
        })
    }
}
