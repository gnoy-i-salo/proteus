//! Freeze only native-completed Fabric memory, with current source authority.
use super::{AuthorizedMemory, CompletedModelContext, MemorySession, RecordedRetrieval};
use crate::{
    corpus_database::RevisionReference,
    ingestion::grants::GrantUse,
    memory::{
        manifest_draft::{ManifestFreezeReceipt, PersonaManifestDraft},
        types::{
            MemoryItem, MemoryItemKind, MemoryItemSupport, ModelComponent, ModelEffort,
            ModelProvider, ModelSettingsEntry, PersonaManifest, SkillDefinition,
        },
    },
};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};
const INVALID: &str = "Манифест не соответствует завершённой подготовке Fabric.";

#[derive(Clone)]
pub(super) struct ManifestMemo {
    hash: String,
    id: String,
    skill_ids: Vec<String>,
    created_at: String,
}
impl ManifestMemo {
    fn new(hash: String, skills: usize, now: i64) -> Result<Self, String> {
        let native_id = |prefix: &str| -> Result<String, String> {
            Ok(format!(
                "{prefix}:{}",
                crate::connections::storage::random_id(24)?
            ))
        };
        Ok(Self {
            hash,
            id: native_id("personaManifest")?,
            skill_ids: (0..skills)
                .map(|_| native_id("memorySkill"))
                .collect::<Result<_, _>>()?,
            created_at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now)
                .ok_or(INVALID)?
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        })
    }
}
impl MemorySession {
    pub fn freeze_manifest(
        &mut self,
        draft: &PersonaManifestDraft,
        now: impl Fn() -> i64,
    ) -> Result<ManifestFreezeReceipt, String> {
        draft.validate()?;
        self.verify_current(&now)?;
        let hash = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(draft).map_err(|_| INVALID)?)
        );
        if self.manifest.as_ref().is_some_and(|memo| memo.hash != hash) {
            return Err("Сессия уже сохранила другой манифест.".into());
        }
        let proof = self.completed_fabric_memory(&draft.context_lease_ids, &now)?;
        let audit = self.completed_context_audit(&draft.context_lease_ids, &now)?;
        verify_models(
            &draft.model_settings,
            ModelComponent::PersonFabric,
            "generation",
            &audit,
        )?;
        let (items, records) = selected_memory(draft, &proof)?;
        crate::memory::research_support::verify(&items, &records, &|| {
            self.scope.guard(&now).map(|_| ())
        })?;
        let time = self.ledger.verify(|| self.scope.checked_time(&now))?;
        let memo = match &self.manifest {
            Some(memo) => memo.clone(),
            None => ManifestMemo::new(hash, draft.skills.len(), time)?,
        };
        let skills: Vec<_> = draft
            .skills
            .iter()
            .zip(&memo.skill_ids)
            .map(|(skill, id)| skill.materialize(id))
            .collect();
        verify_selections(draft, &skills, &items, &records)?;
        let mut seen = HashSet::new();
        let source_refs: Vec<_> = records
            .iter()
            .flat_map(|record| &record.sources)
            .filter_map(|source| {
                let pair = (
                    source.span.document_id.clone(),
                    source.span.revision_id.clone(),
                );
                seen.insert(pair.clone()).then_some(RevisionReference {
                    document_id: pair.0,
                    revision_id: pair.1,
                })
            })
            .collect();
        self.verify_current(&now)?;
        let scope = &self.scope;
        let ledger = RefCell::new(&mut self.ledger);
        let models = RefCell::new(self.models.as_mut().ok_or(INVALID)?);
        let stored = scope
            .sources
            .with_ingestion_database(&scope.operation, |db| {
                let clock = || scope.checked_time(&now);
                let check = || {
                    ledger.borrow_mut().verify(clock)?;
                    models.borrow_mut().verify(&clock)?;
                    Ok(())
                };
                check()?;
                scope.authority.current(db, clock()?, GrantUse::Model)?;
                let pin = scope
                    .authority
                    .pin_index(db, &scope.source.snapshot_id, &check)?;
                if pin.receipt != scope.source {
                    return Err(INVALID.into());
                }
                let research = db.read_research_for_memory_checked(
                    &draft.memory_snapshot_id,
                    &scope.persona_id,
                    &check,
                )?;
                if research.bundle.snapshot.corpus_snapshot_id != scope.source.snapshot_id {
                    return Err(INVALID.into());
                }
                verify_models(
                    &draft.model_settings,
                    ModelComponent::Stalker,
                    "corpus-analysis",
                    &research.publication.model_contexts,
                )?;
                let prepared = db.prepared_snapshot_checked(&scope.source.snapshot_id, &check)?;
                let preparation_version =
                    preparation_version(&prepared.tokenizer_version, &prepared.chunking_version)?;
                let embedding_fingerprint = db
                    .active_vector_receipt(&scope.source.snapshot_id, &check)?
                    .map(|receipt| {
                        crate::local_embedding::runtime::receipt_fingerprint(
                            &receipt.model_fingerprint,
                            &prepared.tokenizer_version,
                            &prepared.chunking_version,
                        )
                        .map_err(crate::local_embedding::model::model_error)
                    })
                    .transpose()?;
                let manifest = PersonaManifest {
                    schema_version: draft.schema_version,
                    profile_snapshot: draft.profile_snapshot.clone(),
                    id: memo.id.clone(),
                    persona_id: scope.persona_id.clone(),
                    profile_version: draft.profile_version.clone(),
                    skill_ids: memo.skill_ids.clone(),
                    exemplar_ids: draft.exemplar_ids.clone(),
                    corpus_snapshot_id: scope.source.snapshot_id.clone(),
                    memory_snapshot_id: draft.memory_snapshot_id.clone(),
                    source_refs,
                    split_version: prepared.split_version,
                    preparation_version,
                    retrieval_version: "nativeMemoryRetrieval:1".into(),
                    prompt_version: draft.prompt_version.clone(),
                    embedding_fingerprint,
                    mode: draft.mode,
                    recall_budget: draft.recall_budget.clone(),
                    model_call_budget: draft.model_call_budget.clone(),
                    model_settings: draft.model_settings.clone(),
                    created_at: memo.created_at.clone(),
                };
                db.freeze_manifest_checked(&manifest, &skills, &check)
            })?;
        // Remember a successful commit even if a final revocation prevents disclosure.
        self.manifest = Some(memo);
        self.verify_current(now)?;
        Ok(ManifestFreezeReceipt {
            schema_version: 1,
            manifest: stored.manifest,
            skills: stored.skills,
        })
    }
}

pub(super) fn preparation_version(tokenizer: &str, chunking: &str) -> Result<String, String> {
    let hash = Sha256::digest(serde_json::to_vec(&(tokenizer, chunking)).map_err(|_| INVALID)?);
    Ok(format!("nativePreparation:1:{hash:x}"))
}
fn verify_models(
    settings: &[ModelSettingsEntry],
    component: ModelComponent,
    operation: &str,
    audit: &[CompletedModelContext],
) -> Result<(), String> {
    let selected = settings
        .iter()
        .find(|entry| entry.component == component)
        .ok_or(INVALID)?;
    let provider = match selected.provider {
        ModelProvider::Codex => "codex",
        ModelProvider::Kimi => "kimi",
    };
    let effort = match selected.requested_effort {
        ModelEffort::Low => "low",
        ModelEffort::Medium => "medium",
        ModelEffort::High => "high",
    };
    if audit.iter().any(|entry| {
        entry.operation != operation
            || entry.provider != provider
            || entry.effort != effort
            || selected.requested_model.as_deref() != Some(entry.model.as_str())
    }) {
        return Err("Настройки манифеста не соответствуют выполненным модельным вызовам.".into());
    }
    Ok(())
}
fn selected_memory(
    draft: &PersonaManifestDraft,
    proof: &[Arc<AuthorizedMemory>],
) -> Result<(Vec<MemoryItem>, Vec<Arc<RecordedRetrieval>>), String> {
    let mut items = Vec::new();
    let mut ids = HashMap::new();
    let mut records = Vec::new();
    let mut receipts = HashSet::new();
    for memory in proof {
        if memory.memory_snapshot_id() != draft.memory_snapshot_id {
            return Err(INVALID.into());
        }
        for item in memory.items() {
            if let Some(previous) = ids.insert(item.id.as_str(), item) {
                if previous != item {
                    return Err(INVALID.into());
                }
            } else {
                items.push(item.clone());
            }
        }
        for record in memory.receipts() {
            if receipts.insert(&record.receipt.id) {
                records.push(record.clone());
            }
        }
    }
    if items.len() > 1000 || records.len() > 1000 {
        return Err(INVALID.into());
    }
    Ok((items, records))
}
fn verify_selections(
    draft: &PersonaManifestDraft,
    skills: &[SkillDefinition],
    items: &[MemoryItem],
    records: &[Arc<RecordedRetrieval>],
) -> Result<(), String> {
    let known: HashMap<_, _> = items.iter().map(|item| (&item.id, item)).collect();
    for exemplar in &draft.exemplar_ids {
        let item = known.get(exemplar).ok_or(INVALID)?;
        if item.kind != MemoryItemKind::StyleExample
            || item.support != MemoryItemSupport::Observed
            || !item.contradiction_ids.is_empty()
        {
            return Err(INVALID.into());
        }
    }
    for skill in skills {
        let cited = skill
            .memory_item_ids
            .iter()
            .map(|id| known.get(id).copied().ok_or(INVALID))
            .collect::<Result<Vec<_>, _>>()?;
        for span in &skill.source_spans {
            if !cited.iter().any(|item| {
                item.source_spans.iter().any(|host| {
                    host.document_id == span.document_id
                        && host.revision_id == span.revision_id
                        && host.start <= span.start
                        && host.end >= span.end
                })
            }) {
                return Err(INVALID.into());
            }
            let mut found = false;
            for source in records.iter().flat_map(|record| &record.sources) {
                if source.span.document_id == span.document_id
                    && source.span.revision_id == span.revision_id
                    && source.span.start <= span.start
                    && source.span.end >= span.end
                {
                    source
                        .text
                        .get(span.start - source.span.start..span.end - source.span.start)
                        .ok_or(INVALID)?;
                    found = true;
                }
            }
            if !found {
                return Err(INVALID.into());
            }
        }
    }
    Ok(())
}
