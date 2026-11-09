//! Session-owned research model bindings. Only native orchestration may configure,
//! consume or complete a context; model actions remain recall/readEpisode only.
use super::MemorySession;
use crate::memory::{
    model_contexts::ModelContexts,
    session::{DispatchInput, InvocationBinding, RecordedRetrieval, ReservedContext},
    types::ModelCallBudget,
};
use std::sync::Arc;
const UNCONFIGURED: &str = "Модельная сессия памяти не настроена.";
impl MemorySession {
    pub fn reserve_context(
        &mut self,
        request: &crate::memory::context_wire::ModelContextRequest,
        now: impl Fn() -> i64,
    ) -> Result<crate::memory::context_wire::ModelContextLease, String> {
        use crate::memory::context_wire::{ContextSources, ContextUsage, ModelContextLease};
        request.validate()?;
        if request.purpose != "research" {
            return Err("Контекст Person требует закреплённого manifest.".into());
        }
        let deadline = self.research_deadline()?;
        let expires_at = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(deadline)
            .ok_or("Неверный срок контекста.")?
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let policy_version = self.scope.policy_version()?.to_owned();
        let records = self.authorized_receipts(&request.receipt_ids, &now)?;
        let reserved = self.reserve_research_context(
            &request.receipt_ids,
            &request.system_prompt,
            &request.task,
            request.format_attempt,
            &now,
        )?;
        let lease = ModelContextLease {
            schema_version: 2,
            id: reserved.id,
            single_use: true,
            persona_id: self.scope.persona_id.clone(),
            input_sha256: reserved.input.identity.input_sha256.clone(),
            input_bytes: reserved.input.identity.input_bytes,
            system_prompt: reserved.input.system_prompt.clone(),
            task: reserved.input.task.clone(),
            corpus_snapshot_id: self.scope.source.snapshot_id.clone(),
            manifest_id: None,
            purpose: "research",
            format_attempt: request.format_attempt,
            policy_version,
            receipt_ids: request.receipt_ids.clone(),
            items: vec![],
            sources: ContextSources(records),
            context_sha256: reserved.input.context_sha256.clone(),
            expires_at,
            usage: ContextUsage {
                model_calls: reserved.usage.used_calls,
                input_bytes: reserved.usage.used_input_bytes,
            },
        };
        self.verify_current(now)?;
        Ok(lease)
    }
    pub fn completed_context_audit(
        &mut self,
        ids: &[String],
        now: impl Fn() -> i64,
    ) -> Result<Vec<super::CompletedModelContext>, String> {
        self.verify_current(&now)?;
        let audit = self
            .models
            .as_mut()
            .ok_or(UNCONFIGURED)?
            .completed_audit(ids, &|| self.scope.guard(&now))?;
        self.verify_current(now)?;
        Ok(audit)
    }
    pub fn verify_research_support(
        &mut self,
        items: &[crate::memory::types::MemoryItem],
        context_ids: &[String],
        now: impl Fn() -> i64,
    ) -> Result<crate::memory::session::VerifiedResearchSupport, String> {
        let receipts = self.completed_context_receipts(context_ids, &now)?;
        let support = crate::memory::research_support::verify(items, &receipts, &|| {
            self.scope.guard(&now).map(|_| ())
        })?;
        self.verify_current(now)?;
        Ok(support)
    }

    pub fn configure_research_model(
        &mut self,
        binding: InvocationBinding,
        budget: ModelCallBudget,
        now: impl Fn() -> i64,
    ) -> Result<(), String> {
        if self.models.is_some() || binding.operation != "corpus-analysis" {
            return Err("Модельная сессия уже настроена или имеет другое назначение.".into());
        }
        budget.validate()?;
        let calls = u32::try_from(budget.max_calls).map_err(|_| "Неверный бюджет модели.")?;
        let started = self.ledger.verify(|| self.scope.guard(&now))?;
        self.models = Some(ModelContexts::new(
            binding,
            calls,
            budget.max_input_bytes,
            budget.max_duration_ms,
            started,
        )?);
        self.verify_current(now)
    }
    pub fn reserve_research_context(
        &mut self,
        ids: &[String],
        system_prompt: &str,
        task: &str,
        format_attempt: u8,
        now: impl Fn() -> i64,
    ) -> Result<ReservedContext, String> {
        if self.models.is_none() {
            return Err(UNCONFIGURED.into());
        }
        let receipts = self.authorized_receipts(ids, &now)?;
        let reserved = self.models.as_mut().ok_or(UNCONFIGURED)?.reserve(
            system_prompt,
            task,
            receipts,
            format_attempt,
            &|| self.scope.guard(&now),
        )?;
        self.verify_current(now)?;
        Ok(reserved)
    }
    pub fn consume_model_context(
        &mut self,
        id: &str,
        dispatch: DispatchInput<'_>,
        now: impl Fn() -> i64,
    ) -> Result<(), String> {
        self.verify_current(&now)?;
        self.models
            .as_mut()
            .ok_or(UNCONFIGURED)?
            .consume(id, dispatch, &|| self.scope.guard(&now))?;
        self.verify_current(now)
    }
    pub fn check_running_context(
        &mut self,
        id: &str,
        invocation_id: &str,
        now: impl Fn() -> i64,
    ) -> Result<(), String> {
        self.verify_current(&now)?;
        self.models
            .as_mut()
            .ok_or(UNCONFIGURED)?
            .check_running(id, invocation_id, &|| self.scope.guard(&now))?;
        self.verify_current(now)
    }
    /// Native dispatch calls this only after the bound CLI invocation succeeds.
    pub fn complete_model_context(
        &mut self,
        id: &str,
        invocation_id: &str,
        now: impl Fn() -> i64,
    ) -> Result<(), String> {
        self.verify_current(&now)?;
        self.models
            .as_mut()
            .ok_or(UNCONFIGURED)?
            .complete(id, invocation_id, &|| self.scope.guard(&now))?;
        self.verify_current(now)
    }
    pub fn completed_context_receipts(
        &mut self,
        ids: &[String],
        now: impl Fn() -> i64,
    ) -> Result<Vec<Arc<RecordedRetrieval>>, String> {
        self.verify_current(&now)?;
        let receipts = self
            .models
            .as_mut()
            .ok_or(UNCONFIGURED)?
            .completed_receipts(ids, &|| self.scope.guard(&now))?;
        self.verify_current(now)?;
        Ok(receipts)
    }
}
