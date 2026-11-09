use super::*;
use crate::memory::session::PersonaSessionInput;
#[path = "memory-persona-context-tests.rs"]
mod context_tests;

fn frozen(
    sources: &Sources,
    snapshot: &crate::ingestion::publication::SnapshotView,
) -> crate::memory::manifest_draft::ManifestFreezeReceipt {
    freeze_persona(sources, snapshot, true)
}
fn freeze_persona(
    sources: &Sources,
    snapshot: &crate::ingestion::publication::SnapshotView,
    bound: bool,
) -> crate::memory::manifest_draft::ManifestFreezeReceipt {
    let mut researcher = session(sources, snapshot, limits()).unwrap();
    let publication = publish_two(&mut researcher);
    drop(researcher);
    let mut fabric = session(sources, snapshot, limits()).unwrap();
    fabric
        .configure_fabric_model(fabric_binding(), fabric_budget(), || 70004)
        .unwrap();
    let lease = fabric
        .reserve_fabric_context(&request(&publication), || 70005)
        .unwrap();
    let mut draft = freeze_draft(&publication.snapshot.id, &lease);
    if bound {
        draft.schema_version = 2;
        draft.profile_snapshot = Some(profile_body());
        draft.profile_version = "person-memory-profile:2".into();
        draft.prompt_version = "person-fabric-memory:2".into();
    }
    complete(&mut fabric, &lease);
    fabric.freeze_manifest(&draft, || 70006).unwrap()
}
fn bind(sources: &Sources, manifest: &str, persona: &str) -> Result<MemorySession, String> {
    let operation = sources
        .begin_ingestion_operation("persona", crate::corpus_database::JobProvider::Telegram)?;
    MemorySession::bind_persona(
        sources.clone(),
        operation,
        PersonaSessionInput {
            job_id: "job",
            persona_id: persona,
            manifest_id: manifest,
            period: crate::corpus_database::search::SearchPeriod {
                from: None,
                to: None,
                include_undated: true,
            },
        },
        || 70007,
    )
}
#[test]
fn persona_admission_pins_native_manifest_model_deadline_and_retrieval_budget() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let receipt = frozen(&sources, &snapshot);
    let mut person = bind(&sources, &receipt.manifest.id, "personA").unwrap();
    assert_eq!(person.persona_manifest(|| 70007).unwrap(), receipt.manifest);
    assert!(person
        .configure_fabric_model(fabric_binding(), fabric_budget(), || 70007)
        .is_err());
    let episode = episode(&sources, &snapshot.id);
    for expected in 1..=receipt.manifest.recall_budget.max_calls {
        person.read_episode_recorded(&episode, || 70008).unwrap();
        assert_eq!(person.usage().0 as u64, expected);
    }
    assert!(person.read_episode_recorded(&episode, || 70008).is_err());
    drop(person);
    let mut fresh = bind(&sources, &receipt.manifest.id, "personA").unwrap();
    assert!(
        fresh.persona_manifest(|| 71007).is_err(),
        "frozen model budget expires before recall budget"
    );
    drop(fresh);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn persona_admission_rejects_foreign_owner_and_live_revocation() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let receipt = frozen(&sources, &snapshot);
    assert!(bind(&sources, &receipt.manifest.id, "foreign").is_err());
    assert!(bind(&sources, "missing", "personA").is_err());
    let mut person = bind(&sources, &receipt.manifest.id, "personA").unwrap();
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    assert!(person.persona_manifest(|| 70008).is_err());
    drop(person);
    assert!(bind(&sources, &receipt.manifest.id, "personA").is_err());
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn persona_admission_rejects_unavailable_model_and_changed_preparation_versions() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let receipt = frozen(&sources, &snapshot);
    for kind in ["split", "preparation", "retrieval", "model", "embedding"] {
        let mut manifest = receipt.manifest.clone();
        manifest.id = format!("altered:{kind}");
        manifest.skill_ids.clear();
        match kind {
            "split" => manifest.split_version = "unknown-split".into(),
            "preparation" => manifest.preparation_version = "nativePreparation:1:wrong".into(),
            "retrieval" => manifest.retrieval_version = "unknown-retrieval".into(),
            "embedding" => manifest.embedding_fingerprint = Some(serde_json::from_value(serde_json::json!({
                "modelId":"missing","modelRevision":"revision","modelSha256":"a".repeat(64),
                "tokenizerSha256":"b".repeat(64),"dimensions":1,"pooling":"mean","normalization":"l2",
                "queryTemplate":"query: ","passageTemplate":"passage: ","chunkingVersion":"chunking:1"
            })).unwrap()),
            _ => {
                manifest
                    .model_settings
                    .iter_mut()
                    .find(|entry| entry.component == ModelComponent::Person)
                    .unwrap()
                    .requested_model = None
            }
        };
        sources
            .with_ingestion_database(&op, |db| {
                db.freeze_manifest_checked(&manifest, &[], &|| Ok(()))
            })
            .unwrap();
        assert!(bind(&sources, &manifest.id, "personA").is_err(), "{kind}");
    }
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn persona_admission_and_active_binding_observe_cancellation_and_index_replacement() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let receipt = frozen(&sources, &snapshot);
    let count = Cell::new(0);
    let open = |cutoff: Option<usize>| {
        count.set(0);
        let operation = sources
            .begin_ingestion_operation("persona", crate::corpus_database::JobProvider::Telegram)
            .unwrap();
        let cancelled = operation.cancellation_handle();
        MemorySession::bind_persona(
            sources.clone(),
            operation,
            PersonaSessionInput {
                job_id: "job",
                persona_id: "personA",
                manifest_id: &receipt.manifest.id,
                period: input(&snapshot).period,
            },
            || {
                count.set(count.get() + 1);
                if cutoff == Some(count.get()) {
                    cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                70007
            },
        )
    };
    let probe = open(None).unwrap();
    let last = count.get();
    drop(probe);
    for cutoff in [1, last / 2, last] {
        assert!(open(Some(cutoff)).is_err(), "{cutoff}");
    }
    let mut person = bind(&sources, &receipt.manifest.id, "personA").unwrap();
    rebuild(
        &sources,
        &op,
        &IndexInput {
            job_id: "job",
            snapshot_id: &snapshot.id,
            max_duration_ms: 60000,
        },
        || 70008,
    )
    .unwrap();
    assert!(person.persona_manifest(|| 70009).is_err());
    drop(person);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn persona_without_embeddings_does_not_adopt_a_later_vector_index() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let receipt = frozen(&sources, &snapshot);
    assert!(receipt.manifest.embedding_fingerprint.is_none());
    let model = Model::new(&|_| Ok(()));
    crate::ingestion::vectors::build(
        &sources,
        &op,
        &VectorBuildInput {
            job_id: "job",
            snapshot_id: &snapshot.id,
            budget: crate::memory::research_workflow::embedding_budget(),
        },
        &model,
        || 70007,
    )
    .unwrap();
    let mut person = bind(&sources, &receipt.manifest.id, "personA").unwrap();
    assert!(crate::memory::research_workflow::configure(
        &mut person,
        crate::memory::research_workflow::ResearchMode::Hybrid,
        Some(&model),
        || 70008
    )
    .is_err());
    assert!(person
        .recall_recorded("ok", None, || 70008)
        .unwrap()
        .receipt
        .embedding_fingerprint
        .is_none());
    drop(person);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn persona_admission_rejects_unbound_legacy_manifest_without_rewriting_it() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let receipt = freeze_persona(&sources, &snapshot, false);
    assert!(bind(&sources, &receipt.manifest.id, "personA").is_err());
    let stored = sources
        .with_ingestion_database(&op, |db| {
            db.read_manifest_checked(&receipt.manifest.id, "personA", &|| Ok(()))
        })
        .unwrap();
    assert_eq!(stored.manifest, receipt.manifest);
    assert_eq!(stored.manifest.schema_version, 1);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
