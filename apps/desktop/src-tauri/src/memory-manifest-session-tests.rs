use super::*;
#[path = "memory-persona-session-tests.rs"]
mod persona_session_tests;
use crate::memory::{
    manifest_draft::{PersonaManifestDraft, SkillDefinitionDraft},
    types::{
        ModelComponent, ModelEffort, ModelProvider, ModelSettingsEntry, RecallBudget, RecallMode,
        SkillSupport,
    },
};

fn freeze_draft(
    memory: &str,
    lease: &crate::memory::context_wire::ModelContextLease,
) -> PersonaManifestDraft {
    PersonaManifestDraft {
        schema_version: 1,
        profile_version: "personFabricMemory:1".into(),
        memory_snapshot_id: memory.into(),
        context_lease_ids: vec![lease.id.clone()],
        exemplar_ids: vec![],
        prompt_version: "fabricMemory:1".into(),
        mode: RecallMode::AgenticRecall,
        recall_budget: RecallBudget {
            max_calls: 3,
            max_candidates: 100,
            max_episodes: 10,
            max_source_bytes: 100000,
            max_context_tokens: 10000,
            max_duration_ms: 60000,
        },
        model_call_budget: fabric_budget(),
        model_settings: [
            ModelComponent::Stalker,
            ModelComponent::PersonFabric,
            ModelComponent::Person,
            ModelComponent::BrainAnalyzator,
        ]
        .into_iter()
        .map(|component| ModelSettingsEntry {
            component,
            provider: ModelProvider::Codex,
            requested_model: Some(
                if component == ModelComponent::PersonFabric {
                    "fabric-model"
                } else {
                    "fixture-model"
                }
                .into(),
            ),
            requested_effort: if component == ModelComponent::PersonFabric {
                ModelEffort::High
            } else {
                ModelEffort::Medium
            },
        })
        .collect(),
        skills: vec![SkillDefinitionDraft {
            draft_id: "suggestion:1".into(),
            trigger: "  Когда просят проверить контекст\r\n".into(),
            inputs: vec![],
            steps: vec!["  Открыть исходную запись é 😀\r\n".into()],
            outcome_check: "Сверить источник".into(),
            support: SkillSupport::Proposed,
            memory_item_ids: vec![lease.items[0].id.clone()],
            source_spans: lease.items[0].source_spans.clone(),
            limitations: vec!["Это предложение, а не наблюдаемый навык.".into()],
        }],
    }
}
fn complete(fabric: &mut MemorySession, lease: &crate::memory::context_wire::ModelContextLease) {
    fabric
        .consume_model_context(
            &lease.id,
            DispatchInput {
                invocation_id: "fabric-freeze",
                binding: &fabric_binding(),
                system_prompt: &lease.system_prompt,
                task: &lease.task,
            },
            || 70005,
        )
        .unwrap();
    fabric
        .complete_model_context(&lease.id, "fabric-freeze", || 70005)
        .unwrap();
}

#[test]
fn freeze_derives_native_identity_versions_sources_and_exact_replay() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut researcher = session(&sources, &snapshot, limits()).unwrap();
    let publication = publish_two(&mut researcher);
    drop(researcher);
    let mut fabric = session(&sources, &snapshot, limits()).unwrap();
    fabric
        .configure_fabric_model(fabric_binding(), fabric_budget(), || 70004)
        .unwrap();
    let lease = fabric
        .reserve_fabric_context(&request(&publication), || 70005)
        .unwrap();
    let draft = freeze_draft(&publication.snapshot.id, &lease);
    assert!(
        fabric.freeze_manifest(&draft, || 70005).is_err(),
        "uncompleted lease cannot freeze"
    );
    complete(&mut fabric, &lease);
    let receipt = fabric.freeze_manifest(&draft, || 70006).unwrap();
    assert_eq!(receipt.schema_version, 1);
    assert_eq!(receipt.manifest.persona_id, "personA");
    assert_eq!(receipt.manifest.corpus_snapshot_id, snapshot.id);
    assert_eq!(receipt.manifest.memory_snapshot_id, publication.snapshot.id);
    assert_eq!(receipt.manifest.created_at, "1970-01-01T00:01:10.006Z");
    assert!(receipt.manifest.id.starts_with("personaManifest:"));
    assert_ne!(receipt.skills[0].id, draft.skills[0].draft_id);
    assert_eq!(receipt.skills[0].steps, draft.skills[0].steps);
    assert_eq!(
        receipt.manifest.skill_ids,
        vec![receipt.skills[0].id.clone()]
    );
    assert!(!receipt.manifest.source_refs.is_empty());
    assert!(receipt
        .manifest
        .preparation_version
        .starts_with("nativePreparation:1:"));
    assert_eq!(
        receipt.manifest.retrieval_version,
        "nativeMemoryRetrieval:1"
    );
    assert_eq!(receipt.manifest.embedding_fingerprint, None);
    assert_eq!(fabric.freeze_manifest(&draft, || 70007).unwrap(), receipt);
    let mut changed = draft.clone();
    changed.prompt_version.push('!');
    assert!(fabric.freeze_manifest(&changed, || 70007).is_err());
    let stored = sources
        .with_ingestion_database(&op, |db| {
            db.read_manifest_checked(&receipt.manifest.id, "personA", &|| Ok(()))
        })
        .unwrap();
    assert_eq!(stored.manifest, receipt.manifest);
    assert_eq!(stored.skills, receipt.skills);
    drop(fabric);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn freeze_rejects_unproved_memory_models_skills_and_exemplars_without_poisoning_retry() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut researcher = session(&sources, &snapshot, limits()).unwrap();
    let publication = publish_two(&mut researcher);
    drop(researcher);
    let mut fabric = session(&sources, &snapshot, limits()).unwrap();
    fabric
        .configure_fabric_model(fabric_binding(), fabric_budget(), || 70004)
        .unwrap();
    let mut selected = request(&publication);
    selected.item_ids.truncate(1);
    let lease = fabric.reserve_fabric_context(&selected, || 70005).unwrap();
    complete(&mut fabric, &lease);
    let draft = freeze_draft(&publication.snapshot.id, &lease);
    for kind in [
        "version",
        "proof",
        "memory",
        "stalker",
        "fabric",
        "unselected",
        "observed",
        "exemplar",
        "utf8",
    ] {
        let mut bad = draft.clone();
        match kind {
            "version" => bad.schema_version = 2,
            "proof" => bad.context_lease_ids.clear(),
            "memory" => bad.memory_snapshot_id = "foreign".into(),
            "stalker" => bad.model_settings[0].requested_model = Some("forged".into()),
            "fabric" => bad.model_settings[1].requested_model = Some("forged".into()),
            "unselected" => {
                bad.skills[0].memory_item_ids = vec![publication.snapshot.item_ids[1].clone()]
            }
            "observed" => bad.skills[0].support = SkillSupport::Observed,
            "exemplar" => bad.exemplar_ids = vec![lease.items[0].id.clone()],
            _ => bad.skills[0].source_spans[0].start += 3,
        }
        assert!(fabric.freeze_manifest(&bad, || 70006).is_err(), "{kind}");
    }
    assert!(fabric.freeze_manifest(&draft, || 70006).is_ok());
    drop(fabric);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn freeze_rechecks_live_permissions_index_and_model_deadline_even_after_commit() {
    for kind in ["revoke", "index", "cancel", "expiry"] {
        let (sources, dir) = local();
        let (op, snapshot, _) = indexed(&sources, &dir, 0);
        let grant = enable_model(&sources, &op);
        let mut researcher = session(&sources, &snapshot, limits()).unwrap();
        let publication = publish_two(&mut researcher);
        drop(researcher);
        let mut fabric = session(&sources, &snapshot, limits()).unwrap();
        fabric
            .configure_fabric_model(fabric_binding(), fabric_budget(), || 70004)
            .unwrap();
        let lease = fabric
            .reserve_fabric_context(&request(&publication), || 70005)
            .unwrap();
        complete(&mut fabric, &lease);
        let draft = freeze_draft(&publication.snapshot.id, &lease);
        let receipt = fabric.freeze_manifest(&draft, || 70006).unwrap();
        match kind {
            "revoke" => {
                sources
                    .with_ingestion_database(&op, |db| db.remove(&grant))
                    .unwrap();
            }
            "index" => {
                rebuild(
                    &sources,
                    &op,
                    &IndexInput {
                        job_id: "job",
                        snapshot_id: &snapshot.id,
                        max_duration_ms: 60000,
                    },
                    || 70007,
                )
                .unwrap();
            }
            "cancel" => fabric.cancel(),
            _ => (),
        }
        assert!(
            fabric
                .freeze_manifest(&draft, || if kind == "expiry" { 71004 } else { 70007 })
                .is_err(),
            "{kind}"
        );
        // A frozen manifest is not permission to disclose it after revocation.
        assert!(receipt.manifest.id.starts_with("personaManifest:"));
        drop(fabric);
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn freeze_discards_receipts_on_midflight_and_final_cancellation_or_expiry() {
    use std::cell::Cell;
    let ticks = Cell::new(0usize);
    let mut total = 0;
    for (at, cancel) in [(0, false), (1, true), (1, false), (2, true), (2, false)] {
        let (sources, dir) = local();
        let (op, snapshot, _) = indexed(&sources, &dir, 0);
        enable_model(&sources, &op);
        let mut researcher = session(&sources, &snapshot, limits()).unwrap();
        let publication = publish_two(&mut researcher);
        drop(researcher);
        let mut fabric = session(&sources, &snapshot, limits()).unwrap();
        fabric
            .configure_fabric_model(fabric_binding(), fabric_budget(), || 70004)
            .unwrap();
        let lease = fabric
            .reserve_fabric_context(&request(&publication), || 70005)
            .unwrap();
        complete(&mut fabric, &lease);
        let draft = freeze_draft(&publication.snapshot.id, &lease);
        ticks.set(0);
        let stop = if at == 1 {
            total / 2
        } else {
            total - usize::from(at != 0)
        };
        let result = fabric.freeze_manifest(&draft, || {
            ticks.set(ticks.get() + 1);
            if at != 0 && cancel && ticks.get() == stop {
                sources.cancel_ingestion_operation("memory").unwrap();
            }
            if at != 0 && !cancel && ticks.get() >= stop {
                71004
            } else {
                70006
            }
        });
        if at == 0 {
            assert!(result.is_ok());
            total = ticks.get();
            assert!(total > 10);
        } else {
            assert!(result.is_err(), "stop {stop}, cancel={cancel}");
            assert!(ticks.get() >= stop);
        }
        drop(fabric);
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn freeze_keeps_observed_procedures_and_verbatim_style_exemplars_from_completed_memory() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut researcher = session(&sources, &snapshot, limits()).unwrap();
    let mut prepared = draft(&mut researcher);
    let own = researcher.recall_recorded("ok", None, || 70003).unwrap();
    assert!(own.sources[0].can_support_own_words);
    let reserved = researcher
        .reserve_research_context(
            std::slice::from_ref(&own.receipt.id),
            "research",
            "own words",
            1,
            || 70003,
        )
        .unwrap();
    let binding = InvocationBinding {
        provider: "codex".into(),
        model: "fixture-model".into(),
        effort: "medium".into(),
        operation: "corpus-analysis".into(),
    };
    researcher
        .consume_model_context(
            &reserved.id,
            DispatchInput {
                invocation_id: "own-research",
                binding: &binding,
                system_prompt: &reserved.input.system_prompt,
                task: &reserved.input.task,
            },
            || 70003,
        )
        .unwrap();
    researcher
        .complete_model_context(&reserved.id, "own-research", || 70003)
        .unwrap();
    prepared.context_lease_ids = vec![reserved.id];
    prepared.analyzed_refs = own
        .sources
        .iter()
        .map(|source| crate::corpus_database::RevisionReference {
            document_id: source.span.document_id.clone(),
            revision_id: source.span.revision_id.clone(),
        })
        .collect();
    prepared.items[0].kind = MemoryItemKind::Procedure;
    prepared.items[0].support = MemoryItemSupport::Observed;
    prepared.items[0].text = own.sources[0].text.clone();
    prepared.items[0].source_spans = vec![own.sources[0].span.clone()];
    prepared.items[0].episode_ids = vec![own.receipt.hits[0].episode_id.clone()];
    let mut style = prepared.items[0].clone();
    style.draft_id = "style:1".into();
    style.kind = MemoryItemKind::StyleExample;
    prepared.items.push(style);
    let publication = researcher.publish_research(&prepared, || 70004).unwrap();
    drop(researcher);
    let mut fabric = session(&sources, &snapshot, limits()).unwrap();
    fabric
        .configure_fabric_model(fabric_binding(), fabric_budget(), || 70004)
        .unwrap();
    let lease = fabric
        .reserve_fabric_context(&request(&publication), || 70005)
        .unwrap();
    complete(&mut fabric, &lease);
    let mut draft = freeze_draft(&publication.snapshot.id, &lease);
    draft.exemplar_ids = vec![lease.items[1].id.clone()];
    draft.skills[0].support = SkillSupport::Observed;
    draft.skills[0].limitations.clear();
    let receipt = fabric.freeze_manifest(&draft, || 70006).unwrap();
    assert_eq!(receipt.manifest.exemplar_ids, draft.exemplar_ids);
    assert_eq!(receipt.skills[0].support, SkillSupport::Observed);
    assert_eq!(receipt.skills[0].source_spans, lease.items[0].source_spans);
    drop(fabric);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn freeze_draft_wire_rejects_caller_identity_missing_fields_and_unknown_versions() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut researcher = session(&sources, &snapshot, limits()).unwrap();
    let publication = publish_two(&mut researcher);
    drop(researcher);
    let mut fabric = session(&sources, &snapshot, limits()).unwrap();
    fabric
        .configure_fabric_model(fabric_binding(), fabric_budget(), || 70004)
        .unwrap();
    let lease = fabric
        .reserve_fabric_context(&request(&publication), || 70005)
        .unwrap();
    let draft = freeze_draft(&publication.snapshot.id, &lease);
    let value = serde_json::to_value(&draft).unwrap();
    serde_json::from_value::<PersonaManifestDraft>(value.clone())
        .unwrap()
        .validate()
        .unwrap();
    for key in value.as_object().unwrap().keys() {
        let mut bad = value.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(
            serde_json::from_value::<PersonaManifestDraft>(bad).is_err(),
            "missing {key}"
        );
    }
    for key in [
        "id",
        "personaId",
        "corpusSnapshotId",
        "sourceRefs",
        "preparationVersion",
        "splitVersion",
        "embeddingFingerprint",
        "createdAt",
        "context_lease_ids",
    ] {
        let mut bad = value.clone();
        bad[key] = serde_json::json!("forged");
        assert!(
            serde_json::from_value::<PersonaManifestDraft>(bad).is_err(),
            "identity {key}"
        );
    }
    for key in ["id", "schemaVersion", "unknown"] {
        let mut bad = value.clone();
        bad["skills"][0][key] = serde_json::json!(1);
        assert!(
            serde_json::from_value::<PersonaManifestDraft>(bad).is_err(),
            "skill {key}"
        );
    }
    for key in ["schemaVersion", "mode"] {
        let mut bad = value.clone();
        bad[key] = serde_json::json!(99);
        assert!(serde_json::from_value::<PersonaManifestDraft>(bad)
            .and_then(|draft| draft.validate().map_err(serde::de::Error::custom))
            .is_err());
    }
    let mut bad = draft.clone();
    bad.skills = vec![draft.skills[0].clone(); 65];
    assert!(bad.validate().is_err());
    let mut bad = draft.clone();
    bad.context_lease_ids.push(bad.context_lease_ids[0].clone());
    assert!(bad.validate().is_err());
    let mut bad = draft;
    bad.model_settings[1] = bad.model_settings[0].clone();
    assert!(bad.validate().is_err());
    drop(fabric);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
