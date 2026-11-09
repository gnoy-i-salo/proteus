//! Person session admission derives its model and budgets from native storage.
use super::{InvocationBinding, MemorySession, Operation, SearchPeriod, SessionInput, Sources};
use crate::{
    corpus_database::{
        memory_store::ManifestPin, search::SearchIndexReceipt, vectors::VectorReceipt,
    },
    ingestion::{grants::GrantUse, index_authority::IndexAuthority},
    memory::{
        budget::RecallLimits,
        types::{self, ModelComponent, ModelEffort, ModelProvider},
    },
};
const INVALID: &str = "Манифест Person не соответствует сохранённой подготовке.";
pub(super) struct PersonaBinding {
    pub(super) pin: ManifestPin,
    pub(super) vectors: Option<VectorReceipt>,
    pub(super) context_memory: Option<std::sync::Arc<super::AuthorizedMemory>>,
    pub(super) simulation: Option<super::persona_simulation::SimulationMemo>,
}
pub struct PersonaSessionInput<'a> {
    pub job_id: &'a str,
    pub persona_id: &'a str,
    pub manifest_id: &'a str,
    pub period: SearchPeriod,
}
impl MemorySession {
    pub fn bind_persona(
        sources: Sources,
        operation: Operation,
        input: PersonaSessionInput<'_>,
        now: impl Fn() -> i64,
    ) -> Result<Self, String> {
        for value in [input.job_id, input.persona_id, input.manifest_id] {
            types::validate_host_id(value, "область Person")?;
        }
        let (persona, index) = load_binding(&sources, &operation, &input, &now)?;
        let manifest = &persona.pin.bundle().manifest;
        let budget = &manifest.recall_budget;
        let selected = manifest
            .model_settings
            .iter()
            .find(|entry| entry.component == ModelComponent::Person)
            .ok_or(INVALID)?;
        let binding = InvocationBinding {
            operation: "persona-reply".into(),
            provider: match selected.provider {
                ModelProvider::Codex => "codex",
                ModelProvider::Kimi => "kimi",
            }
            .into(),
            model: selected
                .requested_model
                .clone()
                .ok_or("Модель Person не закреплена в манифесте.")?,
            effort: match selected.requested_effort {
                ModelEffort::Low => "low",
                ModelEffort::Medium => "medium",
                ModelEffort::High => "high",
            }
            .into(),
        };
        let model_budget = manifest.model_call_budget.clone();
        let mut session = Self::bind(
            sources,
            operation,
            SessionInput {
                job_id: input.job_id,
                snapshot_id: &manifest.corpus_snapshot_id,
                persona_id: input.persona_id,
                period: input.period,
                limits: RecallLimits {
                    max_calls: budget.max_calls as usize,
                    max_candidates: budget.max_candidates as usize,
                    max_episodes: budget.max_episodes as usize,
                    max_source_bytes: budget.max_source_bytes as usize,
                    max_context_tokens: budget.max_context_tokens as usize,
                    max_duration_ms: budget.max_duration_ms as i64,
                },
            },
            &now,
        )?;
        if session.scope.source != index {
            return Err(INVALID.into());
        }
        session.scope.persona = Some(persona);
        session.configure_model(binding, model_budget, "persona-reply", &now)?;
        session.verify_current(now)?;
        Ok(session)
    }
    pub fn persona_manifest(
        &mut self,
        now: impl Fn() -> i64,
    ) -> Result<crate::memory::types::PersonaManifest, String> {
        self.verify_current(&now)?;
        let manifest = self
            .scope
            .persona
            .as_ref()
            .ok_or(INVALID)?
            .pin
            .bundle()
            .manifest
            .clone();
        self.verify_current(now)?;
        Ok(manifest)
    }
}

fn load_binding(
    sources: &Sources,
    operation: &Operation,
    input: &PersonaSessionInput<'_>,
    now: &dyn Fn() -> i64,
) -> Result<(PersonaBinding, SearchIndexReceipt), String> {
    let started = now();
    let deadline = started
        .checked_add(60000)
        .filter(|_| started >= 0)
        .ok_or(INVALID)?;
    operation.check()?;
    sources.with_ingestion_database(operation, |db| {
        let authority =
            IndexAuthority::resolve(db, operation, input.job_id, Some(input.persona_id), started)?;
        let check = || {
            operation.check()?;
            let time = now();
            operation.check()?;
            if time < started || time >= deadline || time >= authority.deadline {
                return Err(INVALID.into());
            }
            Ok(())
        };
        check()?;
        authority.current(db, started, GrantUse::Model)?;
        let pin = db.pin_manifest_checked(input.manifest_id, input.persona_id, &check)?;
        let manifest = &pin.bundle().manifest;
        let index = authority.pin_index(db, &manifest.corpus_snapshot_id, &check)?;
        let prepared = db.prepared_snapshot_checked(&manifest.corpus_snapshot_id, &check)?;
        if manifest.split_version != prepared.split_version
            || manifest.preparation_version
                != super::manifest_session::preparation_version(
                    &prepared.tokenizer_version,
                    &prepared.chunking_version,
                )?
            || manifest.retrieval_version != "nativeMemoryRetrieval:1"
        {
            return Err(INVALID.into());
        }
        let vectors = if let Some(expected) = &manifest.embedding_fingerprint {
            let vectors = db
                .active_vector_receipt(&manifest.corpus_snapshot_id, &check)?
                .ok_or(INVALID)?;
            let fingerprint = crate::local_embedding::runtime::receipt_fingerprint(
                &vectors.model_fingerprint,
                &prepared.tokenizer_version,
                &prepared.chunking_version,
            )
            .map_err(crate::local_embedding::model::model_error)?;
            if vectors.source != index.receipt || fingerprint != *expected {
                return Err(INVALID.into());
            }
            Some(vectors)
        } else {
            None
        };
        check()?;
        Ok((
            PersonaBinding {
                pin,
                vectors,
                context_memory: None,
                simulation: None,
            },
            index.receipt,
        ))
    })
}
