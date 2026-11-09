//! Host-bound retrieval state. Scope and limits are chosen once by native
//! orchestration; model actions can only select a query or an episode ID.
#[path = "memory-catalog-session.rs"]
mod catalog_session;
pub use catalog_session::ResearchCatalogResponse;
#[path = "memory-fabric-session.rs"]
mod fabric_session;
pub use fabric_session::FabricContextRequest;
#[path = "memory-manifest-session.rs"]
mod manifest_session;
#[path = "memory-model-session.rs"]
mod model_session;
#[path = "memory-persona-context.rs"]
mod persona_context;
#[path = "memory-persona-session.rs"]
mod persona_session;
#[path = "memory-persona-simulation.rs"]
mod persona_simulation;
pub use persona_session::PersonaSessionInput;
#[path = "memory-published-read.rs"]
mod published_read;
pub use published_read::AuthorizedMemory;
#[path = "memory-publication-session.rs"]
mod publication_session;
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
pub use receipts::{ReceiptHit, RecordedRetrieval, RetrievalReceipt};
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
    publication: Option<publication_session::PublicationMemo>,
    manifest: Option<manifest_session::ManifestMemo>,
}
struct MemoryScope {
    deadline: i64,
    sources: Sources,
    operation: Operation,
    authority: IndexAuthority,
    source: SearchIndexReceipt,
    persona_id: String,
    period: SearchPeriod,
    persona: Option<persona_session::PersonaBinding>,
    hybrid: Option<(
        crate::corpus_database::vectors::VectorReceipt,
        crate::local_embedding::runtime::EmbeddingBudget,
    )>,
}
impl MemorySession {
    pub(super) fn uses_hybrid(&self) -> bool {
        self.scope.hybrid.is_some()
    }
    pub(super) fn research_deadline(&self) -> Result<i64, String> {
        Ok(self.scope.deadline.min(
            self.models
                .as_ref()
                .ok_or("Модельная сессия памяти не настроена.")?
                .deadline_ms(),
        ))
    }
    pub(super) fn research_descriptor(
        &mut self,
        now: impl Fn() -> i64,
    ) -> Result<super::research_workflow::ResearchSession, String> {
        self.verify_current(&now)?;
        let deadline = self.research_deadline()?;
        let value = super::research_workflow::ResearchSession {
            schema_version: 1,
            id: String::new(),
            persona_id: self.scope.persona_id.clone(),
            corpus_snapshot_id: self.scope.source.snapshot_id.clone(),
            policy_version: self.scope.policy_version()?.into(),
            mode: if self.scope.hybrid.is_some() {
                "hybrid"
            } else {
                "lexical"
            },
            index_generation: self.scope.source.index_generation,
            expires_at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(deadline)
                .ok_or("Неверный срок сессии.")?
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        };
        self.verify_current(now)?;
        Ok(value)
    }
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
                persona: None,
                hybrid: None,
            },
            ledger,
            history: Default::default(),
            models: None,
            publication: None,
            manifest: None,
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
    // Use while the caller already holds the source gate; never reacquire it.
    fn checked_time(&self, now: &dyn Fn() -> i64) -> Result<i64, String> {
        self.operation.check()?;
        let time = now();
        self.operation.check()?;
        if time < 0 || time >= self.deadline {
            return Err("Разрешение источника истекло.".into());
        }
        Ok(time)
    }
    fn policy_version(&self) -> Result<&str, String> {
        let policy = &self
            .authority
            .bindings
            .first()
            .ok_or("Нет разрешённых привязок.")?
            .policy_version;
        if self
            .authority
            .bindings
            .iter()
            .any(|binding| &binding.policy_version != policy)
        {
            return Err("Несовместимые политики квитанции поиска.".into());
        }
        Ok(policy)
    }
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
            if let Some(persona) = &self.persona {
                let check = || self.checked_time(now_ms).map(|_| ());
                db.verify_manifest_pin(&persona.pin, &check)?;
                if let Some(vectors) = &persona.vectors {
                    if db
                        .active_vector_receipt(&self.source.snapshot_id, &check)?
                        .as_ref()
                        != Some(vectors)
                    {
                        return Err("Векторный индекс закреплённой персоны изменился.".into());
                    }
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
