use super::*;
use crate::memory::context_wire::ModelContextRequest;
#[path = "memory-persona-workflow-tests.rs"]
mod workflow_tests;

fn context_request(
    receipt: &crate::memory::manifest_draft::ManifestFreezeReceipt,
) -> ModelContextRequest {
    ModelContextRequest {
        schema_version: 2,
        purpose: "personaReply".into(),
        manifest_id: Some(receipt.manifest.id.clone()),
        system_prompt: "  Отвечай от первого лица\r\n".into(),
        task: "\u{feff}Точная задача é 😀\r\n".into(),
        receipt_ids: vec![],
        memory_item_ids: receipt.skills[0].memory_item_ids.clone(),
        format_attempt: 1,
    }
}
fn person_binding() -> InvocationBinding {
    InvocationBinding {
        provider: "codex".into(),
        model: "fixture-model".into(),
        effort: "medium".into(),
        operation: "persona-reply".into(),
    }
}

#[test]
fn persona_context_rejects_foreign_scope_and_discards_cancelled_reservations() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let receipt = frozen(&sources, &snapshot);
    let mut other = session(&sources, &snapshot, limits()).unwrap();
    let foreign = other.recall_recorded("ok", None, || 70007).unwrap();
    drop(other);
    let mut person = bind(&sources, &receipt.manifest.id, "personA").unwrap();
    for kind in ["manifest", "receipt", "item", "research"] {
        let mut request = context_request(&receipt);
        match kind {
            "manifest" => request.manifest_id = Some("foreign".into()),
            "receipt" => request.receipt_ids = vec![foreign.receipt.id.clone()],
            "item" => request.memory_item_ids = vec!["invented".into()],
            _ => {
                request.purpose = "research".into();
                request.manifest_id = None;
                request.memory_item_ids.clear();
            }
        }
        assert!(
            person.reserve_context(&request, || 70008).is_err(),
            "{kind}"
        );
    }
    let request = context_request(&receipt);
    let count = Cell::new(0usize);
    person
        .reserve_context(&request, || {
            count.set(count.get() + 1);
            70008
        })
        .unwrap();
    let total = count.get();
    drop(person);
    for cutoff in [1, total / 2, total] {
        let mut person = bind(&sources, &receipt.manifest.id, "personA").unwrap();
        count.set(0);
        assert!(
            person
                .reserve_context(&request, || {
                    count.set(count.get() + 1);
                    if count.get() == cutoff {
                        sources.cancel_ingestion_operation("persona").unwrap();
                    }
                    70008
                })
                .is_err(),
            "cancel at {cutoff}"
        );
        assert!(person.reserve_context(&request, || 70008).is_err());
    }
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn persona_with_pinned_embeddings_never_falls_back_before_runtime_configuration() {
    use crate::local_embedding::{
        runtime::{E5Runtime, ModelFiles},
        tokenizer::E5Tokenizer,
    };
    let cache = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.cache/models");
    let files = ModelFiles {
        weights: cache.join("e5-model.safetensors"),
        config: cache.join("e5-config.json"),
        tokenizer: cache.join("e5-tokenizer.json"),
    };
    let tokenizer = E5Tokenizer::load(&files.tokenizer).unwrap();
    let (sources, dir) = local();
    let (op, snapshot) = prepared_with_tokenizer(&sources, &dir, 0, &tokenizer);
    rebuild(
        &sources,
        &op,
        &IndexInput {
            job_id: "job",
            snapshot_id: &snapshot.id,
            max_duration_ms: 60000,
        },
        || 70001,
    )
    .unwrap();
    enable_model(&sources, &op);
    let budget = crate::memory::research_workflow::embedding_budget();
    let model = E5Runtime::load(&files, &budget, &|| Ok(())).unwrap();
    build(
        &sources,
        &op,
        &VectorBuildInput {
            job_id: "job",
            snapshot_id: &snapshot.id,
            budget,
        },
        &model,
        || 70002,
    )
    .unwrap();
    let receipt = frozen(&sources, &snapshot);
    assert!(receipt.manifest.embedding_fingerprint.is_some());
    let mut person = bind(&sources, &receipt.manifest.id, "personA").unwrap();
    let request = context_request(&receipt);
    assert!(
        person.reserve_context(&request, || 70008).is_err(),
        "must configure the pinned runtime before issuing a context"
    );
    assert!(
        person.recall_recorded("контекст", None, || 70008).is_err(),
        "never downgrade hybrid to lexical"
    );
    crate::memory::research_workflow::configure(
        &mut person,
        crate::memory::research_workflow::ResearchMode::Hybrid,
        Some(&model),
        || 70008,
    )
    .unwrap();
    let lease = person.reserve_context(&request, || 70008).unwrap();
    assert_eq!(
        lease.manifest_id.as_deref(),
        Some(receipt.manifest.id.as_str())
    );
    let record = person
        .recall_recorded("контекст", Some(&model), || 70008)
        .unwrap();
    assert_eq!(record.receipt.mode, "hybrid");
    assert_eq!(
        record.receipt.embedding_fingerprint,
        receipt.manifest.embedding_fingerprint
    );
    drop(person);
    let sessions = crate::memory::registry::MemorySessions::default();
    let input: crate::memory::persona_workflow::OpenPersonaRequest =
        serde_json::from_value(serde_json::json!({
            "schemaVersion":1,"operationId":"hybrid-persona","jobId":"job","personaId":"personA",
            "manifestId":receipt.manifest.id,"period":{"from":null,"to":null,"includeUndated":true}
        }))
        .unwrap();
    assert!(
        crate::memory::persona_workflow::open(&sources, &sessions, &input, None, || 70007).is_err()
    );
    let prepared = crate::memory::persona_workflow::prepare(&sources, &input, || 70007).unwrap();
    assert!(crate::memory::persona_workflow::admit(prepared, &sessions, || 70007).is_err());
    let opened =
        crate::memory::persona_workflow::open(&sources, &sessions, &input, Some(&model), || 70007)
            .unwrap();
    assert_eq!(opened.session.mode, "hybrid");
    assert_eq!(opened.manifest, receipt.manifest);
    sessions.close(&opened.session.id).unwrap();
    drop(sessions);
    drop(model);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn persona_context_uses_pinned_memory_and_exact_single_use_model_input() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let receipt = frozen(&sources, &snapshot);
    let mut person = bind(&sources, &receipt.manifest.id, "personA").unwrap();
    let record = person.recall_recorded("ok", None, || 70008).unwrap();
    let mut request = context_request(&receipt);
    request.receipt_ids = vec![record.receipt.id.clone()];
    let lease = person.reserve_context(&request, || 70008).unwrap();
    assert_eq!(lease.purpose, "personaReply");
    assert_eq!(lease.manifest_id, request.manifest_id);
    assert_eq!(lease.system_prompt, request.system_prompt);
    assert!(lease.task.starts_with(&request.task));
    assert_eq!(lease.items.len(), request.memory_item_ids.len());
    assert_eq!(lease.items[0].id, request.memory_item_ids[0]);
    assert!(lease.receipt_ids.contains(&record.receipt.id));
    assert_eq!(
        lease
            .receipt_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        lease.receipt_ids.len()
    );
    let value = serde_json::to_value(&lease).unwrap();
    assert!(value["sources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["text"] == "  контекст\r\n"));
    let identity =
        crate::memory::model_input::model_input_identity(&lease.system_prompt, &lease.task)
            .unwrap();
    assert_eq!(lease.input_sha256, identity.input_sha256);
    assert_eq!(lease.input_bytes, identity.input_bytes);
    assert!(person
        .completed_context_audit(std::slice::from_ref(&lease.id), || 70008)
        .is_err());
    for binding in [
        fabric_binding(),
        InvocationBinding {
            model: "forged".into(),
            ..person_binding()
        },
    ] {
        assert!(person
            .consume_model_context(
                &lease.id,
                DispatchInput {
                    invocation_id: "wrong-person-model",
                    binding: &binding,
                    system_prompt: &lease.system_prompt,
                    task: &lease.task,
                },
                || 70008
            )
            .is_err());
    }
    let binary = super::super::super::super::super::super::cli_memory_dispatch_tests::fixture(&dir);
    let run = dir.join("person-run");
    std::fs::create_dir(&run).unwrap();
    let cli_request = serde_json::from_value(serde_json::json!({
        "id":"person-call","provider":"codex","model":"fixture-model","effort":"medium",
        "operation":"persona-reply","sourceContext":true,"systemPrompt":lease.system_prompt,
        "task":lease.task,"memoryContext":{"sessionId":"person-session","leaseId":lease.id}
    }))
    .unwrap();
    crate::cli::execute_fixture(
        cli_request,
        &run,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        |_, _, _| {},
        &binary,
        Some(&mut person),
        &|| 70008,
    )
    .unwrap();
    person
        .check_completed_model_output(&lease.id, "person-call", "fixture answer", || 70008)
        .unwrap();
    assert!(person
        .check_completed_model_output(&lease.id, "person-call", "caller-invented answer", || 70008)
        .is_err());
    assert_eq!(
        std::fs::read_to_string(run.join("received.txt")).unwrap(),
        format!("{}\n\nINPUT DATA:\n{}", lease.system_prompt, lease.task)
    );
    assert!(person
        .consume_model_context(
            &lease.id,
            DispatchInput {
                invocation_id: "person-replay",
                binding: &person_binding(),
                system_prompt: &lease.system_prompt,
                task: &lease.task,
            },
            || 70008
        )
        .is_err());
    let audit = person
        .completed_context_audit(std::slice::from_ref(&lease.id), || 70008)
        .unwrap();
    assert_eq!(audit[0].operation, "persona-reply");
    assert_eq!(audit[0].context_sha256, lease.context_sha256);
    assert!(person
        .completed_fabric_memory(std::slice::from_ref(&lease.id), || 70008)
        .is_err());
    request.format_attempt = 2;
    let repair = person.reserve_context(&request, || 70008).unwrap();
    assert_ne!(repair.id, lease.id);
    assert_eq!(repair.context_sha256, lease.context_sha256);
    assert_eq!(repair.usage.model_calls, 2);
    assert!(person.reserve_context(&request, || 70008).is_err());
    drop(person);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn fixed_persona_context_reuses_evidence_and_rejects_new_selections_and_revocation() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let mut receipt = frozen(&sources, &snapshot);
    receipt.manifest.id = "fixed:manifest".into();
    receipt.manifest.mode = RecallMode::FixedContext;
    receipt.manifest.skill_ids.clear();
    sources
        .with_ingestion_database(&op, |db| {
            db.freeze_manifest_checked(&receipt.manifest, &[], &|| Ok(()))
        })
        .unwrap();
    let mut person = bind(&sources, &receipt.manifest.id, "personA").unwrap();
    let mut request = context_request(&receipt);
    assert!(
        person.recall_recorded("ok", None, || 70008).is_err(),
        "fixed context must reject model retrieval actions"
    );
    assert!(
        person.research_catalog(1, || 70008).is_err(),
        "Person must not expose research catalog"
    );
    assert!(person
        .read_episode_action(&episode(&sources, &snapshot.id), || 70008)
        .is_err());
    let first = person.reserve_context(&request, || 70008).unwrap();
    let usage = person.usage();
    let mut changed = context_request(&receipt);
    changed.memory_item_ids.clear();
    assert!(person.reserve_context(&changed, || 70008).is_err());
    changed = context_request(&receipt);
    changed.receipt_ids = first.receipt_ids.clone();
    assert!(person.reserve_context(&changed, || 70008).is_err());
    request.format_attempt = 2;
    request.task.push_str("Исправь формат.");
    let second = person.reserve_context(&request, || 70008).unwrap();
    assert_eq!(first.context_sha256, second.context_sha256);
    assert_eq!(first.receipt_ids, second.receipt_ids);
    assert_eq!(
        usage,
        person.usage(),
        "fixed context must not rehydrate and charge sources twice"
    );
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    assert!(person
        .consume_model_context(
            &second.id,
            DispatchInput {
                invocation_id: "revoked",
                binding: &person_binding(),
                system_prompt: &second.system_prompt,
                task: &second.task,
            },
            || 70008
        )
        .is_err());
    drop(person);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
