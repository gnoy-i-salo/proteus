//! Task-free Fabric contexts: rehydrate authorized memory, append native evidence
//! and bind exact input to the separately chosen component model. This native
//! scenario does not treat a frontend copy of memory as a source of authority.
use super::{AuthorizedMemory, InvocationBinding, MemorySession};
use crate::memory::{
    context_wire::{ContextSources, ContextUsage, ModelContextLease},
    types::{self, ModelCallBudget},
};
use serde::Deserialize;
use std::sync::Arc;
const INVALID: &str = "Неверная модельная сессия или запрос Person Fabric.";
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FabricContextRequest {
    pub schema_version: u8,
    pub memory_snapshot_id: String,
    pub item_ids: Vec<String>,
    pub system_prompt: String,
    pub task: String,
    pub format_attempt: u8,
}
impl FabricContextRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || ![1, 2].contains(&self.format_attempt)
            || self.system_prompt.len() > 40_000
            || self.task.len() > 200_000
        {
            return Err(INVALID.into());
        }
        types::validate_host_id(&self.memory_snapshot_id, "снимок памяти Fabric")?;
        types::validate_unique_host_ids(&self.item_ids, "элемент памяти Fabric", 1000)
    }
}
impl MemorySession {
    pub fn configure_fabric_model(
        &mut self,
        binding: InvocationBinding,
        budget: ModelCallBudget,
        now: impl Fn() -> i64,
    ) -> Result<(), String> {
        self.configure_model(binding, budget, "generation", now)
    }
    pub fn reserve_fabric_context(
        &mut self,
        request: &FabricContextRequest,
        now: impl Fn() -> i64,
    ) -> Result<ModelContextLease, String> {
        request.validate()?;
        if self
            .models
            .as_ref()
            .is_none_or(|models| models.operation() != "generation")
        {
            return Err(INVALID.into());
        }
        self.verify_current(&now)?;
        let memory = Arc::new(self.read_published_memory(
            &request.memory_snapshot_id,
            &request.item_ids,
            &now,
        )?);
        let reserved = self.models.as_mut().ok_or(INVALID)?.reserve_memory(
            &request.system_prompt,
            &request.task,
            memory.clone(),
            request.format_attempt,
            &|| self.scope.guard(&now),
        )?;
        let expires_at =
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(self.research_deadline()?)
                .ok_or(INVALID)?
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
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
            purpose: "fabric",
            format_attempt: request.format_attempt,
            policy_version: self.scope.policy_version()?.into(),
            receipt_ids: memory
                .receipts()
                .iter()
                .map(|record| record.receipt.id.clone())
                .collect(),
            items: memory.items().to_vec(),
            sources: ContextSources(memory.receipts().to_vec()),
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
    pub fn completed_fabric_memory(
        &mut self,
        ids: &[String],
        now: impl Fn() -> i64,
    ) -> Result<Vec<Arc<AuthorizedMemory>>, String> {
        self.verify_current(&now)?;
        if self
            .models
            .as_ref()
            .is_none_or(|models| models.operation() != "generation")
        {
            return Err(INVALID.into());
        }
        let memory = self
            .models
            .as_mut()
            .ok_or(INVALID)?
            .completed_memory(ids, &|| self.scope.guard(&now))?;
        self.verify_current(now)?;
        Ok(memory)
    }
}
