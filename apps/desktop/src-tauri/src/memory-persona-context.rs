//! Native Person context assembly. Persisted memory is rehydrated once for an
//! unchanged selection; current grants, immutable identities and deadlines still
//! guard every reservation and CLI dispatch. Only agentic mode adds live receipts.
use super::{AuthorizedMemory, MemorySession};
use crate::memory::{
    context_wire::{ContextSources, ContextUsage, ModelContextLease, ModelContextRequest},
    types::RecallMode,
};
use std::{collections::HashSet, sync::Arc};
const INVALID: &str = "Контекст Person не соответствует закреплённому манифесту.";

impl MemorySession {
    pub(in crate::memory) fn verify_persona_runtime(&self) -> Result<(), String> {
        if let Some(persona) = &self.scope.persona {
            if persona.vectors.as_ref() != self.scope.hybrid.as_ref().map(|(vectors, _)| vectors) {
                return Err("Закреплённая модель поиска Person ещё не подключена.".into());
            }
        }
        Ok(())
    }
    pub(super) fn authorize_recall_action(&mut self, now: impl Fn() -> i64) -> Result<(), String> {
        let Some(persona) = &self.scope.persona else {
            // Keep existing research budget rejection before any clock or I/O.
            return Ok(());
        };
        if persona.pin.bundle().manifest.mode != RecallMode::AgenticRecall {
            return Err("Поиск недоступен в режиме фиксированного контекста.".into());
        }
        self.verify_persona_runtime()?;
        self.verify_current(now)
    }
    /// Model-facing episode action; native baseline rehydration uses the internal
    /// recorded reader so fixed-context can assemble its initial evidence.
    pub fn read_episode_action(
        &mut self,
        id: &str,
        now: impl Fn() -> i64,
    ) -> Result<Arc<super::RecordedRetrieval>, String> {
        self.authorize_recall_action(&now)?;
        self.read_episode_recorded(id, now)
    }
    pub(super) fn reserve_persona_context(
        &mut self,
        request: &ModelContextRequest,
        now: impl Fn() -> i64,
    ) -> Result<ModelContextLease, String> {
        self.verify_persona_runtime()?;
        self.verify_current(&now)?;
        let persona = self.scope.persona.as_ref().ok_or(INVALID)?;
        let manifest = &persona.pin.bundle().manifest;
        if request.manifest_id.as_deref() != Some(&manifest.id)
            || self
                .models
                .as_ref()
                .is_none_or(|m| m.operation() != "persona-reply")
            || (manifest.mode == RecallMode::FixedContext && !request.receipt_ids.is_empty())
        {
            return Err(INVALID.into());
        }
        let manifest_id = manifest.id.clone();
        let memory_snapshot_id = manifest.memory_snapshot_id.clone();
        let fixed = manifest.mode == RecallMode::FixedContext;
        let cached = persona.context_memory.clone();
        let unchanged = cached.as_ref().is_some_and(|memory| {
            memory
                .items()
                .iter()
                .map(|item| &item.id)
                .eq(request.memory_item_ids.iter())
        });
        if fixed && cached.is_some() && !unchanged {
            return Err(INVALID.into());
        }
        // Reject unknown/cross-session receipts before any reference reads.
        let extra = self.authorized_receipts(&request.receipt_ids, &now)?;
        let memory = if unchanged {
            cached.ok_or(INVALID)?
        } else {
            Arc::new(self.read_published_memory(
                &memory_snapshot_id,
                &request.memory_item_ids,
                &now,
            )?)
        };
        let mut receipts = memory.receipts().to_vec();
        let mut seen: HashSet<_> = receipts.iter().map(|r| r.receipt.id.clone()).collect();
        for record in extra {
            if seen.insert(record.receipt.id.clone()) {
                receipts.push(record);
            }
        }
        let combined = Arc::new(AuthorizedMemory {
            memory_snapshot_id,
            items: memory.items().to_vec(),
            receipts,
        });
        let reserved = self.models.as_mut().ok_or(INVALID)?.reserve_memory(
            &request.system_prompt,
            &request.task,
            combined.clone(),
            request.format_attempt,
            &|| self.scope.guard(&now),
        )?;
        // Store only the base selection, never agentic additions. Fixed contexts
        // cannot change the selection after their first successful reservation.
        self.scope.persona.as_mut().ok_or(INVALID)?.context_memory = Some(memory);
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
            manifest_id: Some(manifest_id),
            purpose: "personaReply",
            format_attempt: request.format_attempt,
            policy_version: self.scope.policy_version()?.into(),
            receipt_ids: combined
                .receipts()
                .iter()
                .map(|r| r.receipt.id.clone())
                .collect(),
            items: combined.items().to_vec(),
            sources: ContextSources(combined.receipts().to_vec()),
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
}
