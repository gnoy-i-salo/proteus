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
