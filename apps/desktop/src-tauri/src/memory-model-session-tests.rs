use super::*;
use crate::memory::session::{DispatchInput, InvocationBinding};
use crate::memory::types::ModelCallBudget;
fn model_binding() -> InvocationBinding {
    InvocationBinding {
        provider: "codex".into(),
        model: "fixture-model".into(),
        effort: "medium".into(),
        operation: "corpus-analysis".into(),
    }
}
fn model_budget() -> ModelCallBudget {
    ModelCallBudget {
        max_calls: 2,
        max_input_bytes: 100000,
        max_duration_ms: 1000,
    }
}
#[test]
fn wire_lease_preserves_native_input_sources_usage_and_both_deadlines() {
    use crate::memory::context_wire::ModelContextRequest;
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    for (duration, expected) in [(1000, 71003), (100000, 130002)] {
        let mut bound = session(&sources, &snapshot, limits()).unwrap();
        let mut budget = model_budget();
        budget.max_duration_ms = duration;
        bound
            .configure_research_model(model_binding(), budget, || 70003)
            .unwrap();
        let receipt = bound.recall_recorded("контекст", None, || 70003).unwrap();
        let mut request = ModelContextRequest {
            schema_version: 2,
            purpose: "research".into(),
            manifest_id: None,
            system_prompt: "  Инструкция\r\n".into(),
            task: "\u{feff}Задача\r\n  ".into(),
            receipt_ids: vec![receipt.receipt.id.clone()],
            memory_item_ids: vec![],
            format_attempt: 2,
        };
        request.purpose = "personaReply".into();
        request.manifest_id = Some("foreign".into());
        assert!(bound.reserve_context(&request, || 70003).is_err());
        request.purpose = "research".into();
        request.manifest_id = None;
        let lease = bound.reserve_context(&request, || 70003).unwrap();
        assert_eq!(lease.system_prompt, request.system_prompt);
        assert!(lease.task.starts_with(&request.task));
        assert_eq!(lease.persona_id, "personA");
        assert_eq!(lease.corpus_snapshot_id, snapshot.id);
        assert_eq!(lease.format_attempt, 2);
        assert_eq!(lease.usage.model_calls, 1);
        assert_eq!(lease.usage.input_bytes, lease.input_bytes as u64);
        let identity =
            crate::memory::model_input::model_input_identity(&lease.system_prompt, &lease.task)
                .unwrap();
        assert_eq!(lease.input_sha256, identity.input_sha256);
        assert_eq!(lease.input_bytes, identity.input_bytes);
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(&lease.expires_at)
                .unwrap()
                .timestamp_millis(),
            expected
        );
        let wire = serde_json::to_value(&lease).unwrap();
        assert_eq!(wire["schemaVersion"], 2);
        assert_eq!(wire["singleUse"], true);
        assert_eq!(wire["purpose"], "research");
        assert_eq!(wire["manifestId"], serde_json::Value::Null);
        assert_eq!(
            wire["sources"],
            serde_json::to_value(&receipt.sources).unwrap()
        );
        assert_eq!(wire["sources"][0]["text"], "  контекст\r\n");
        assert_eq!(wire["items"], serde_json::json!([]));
        assert_eq!(wire["receiptIds"], serde_json::json!(request.receipt_ids));
        assert_eq!(lease.policy_version, receipt.receipt.policy_version);
        bound
            .consume_model_context(
                &lease.id,
                DispatchInput {
                    invocation_id: "wire-call",
                    binding: &model_binding(),
                    system_prompt: &lease.system_prompt,
                    task: &lease.task,
                },
                || 70003,
            )
            .unwrap();
        assert!(bound
            .consume_model_context(
                &lease.id,
                DispatchInput {
                    invocation_id: "wire-call-2",
                    binding: &model_binding(),
                    system_prompt: &lease.system_prompt,
                    task: &lease.task,
                },
                || 70003
            )
            .is_err());
    }
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    bound
        .configure_research_model(model_binding(), model_budget(), || 70003)
        .unwrap();
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    let request = ModelContextRequest {
        schema_version: 2,
        purpose: "research".into(),
        manifest_id: None,
        system_prompt: String::new(),
        task: String::new(),
        receipt_ids: vec![],
        memory_item_ids: vec![],
        format_attempt: 1,
    };
    assert!(bound.reserve_context(&request, || 70003).is_err());
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn model_context_binds_native_sources_until_completed_and_rejects_revoked_proof() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let receipt = bound.recall_recorded("контекст", None, || 70003).unwrap();
    let binding = model_binding();
    bound
        .configure_research_model(binding.clone(), model_budget(), || 70003)
        .unwrap();
    assert!(bound
        .configure_research_model(binding.clone(), model_budget(), || panic!(
            "model budget reset"
        ))
        .is_err());
    let reserved = bound
        .reserve_research_context(
            std::slice::from_ref(&receipt.receipt.id),
            "prompt\r\n",
            "задача",
            1,
            || 70003,
        )
        .unwrap();
    let ids = [reserved.id.clone()];
    assert!(bound.completed_context_audit(&ids, || 70003).is_err());
    assert_eq!(reserved.input.system_prompt, "prompt\r\n");
    assert!(bound.completed_context_receipts(&ids, || 70003).is_err());
    bound
        .consume_model_context(
            &reserved.id,
            DispatchInput {
                invocation_id: "call-1",
                binding: &binding,
                system_prompt: &reserved.input.system_prompt,
                task: &reserved.input.task,
            },
            || 70003,
        )
        .unwrap();
    assert!(bound.completed_context_receipts(&ids, || 70003).is_err());
    // This test signals completion; production must call only after successful native CLI dispatch.
    bound
        .complete_model_context(&reserved.id, "call-1", || 70003)
        .unwrap();
    let proof = bound.completed_context_receipts(&ids, || 70003).unwrap();
    let audit = bound.completed_context_audit(&ids, || 70003).unwrap();
    assert_eq!(audit[0].receipt_ids, vec![receipt.receipt.id.clone()]);
    assert_eq!(audit[0].input_sha256, reserved.input.identity.input_sha256);
    assert_eq!(audit[0].completed_at_ms, 70003);
    assert_eq!(
        serde_json::to_value(&*proof[0]).unwrap(),
        serde_json::to_value(&*receipt).unwrap()
    );
    assert_eq!(proof[0].sources[0].text, "  контекст\r\n");
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    assert!(bound.completed_context_receipts(&ids, || 70004).is_err());
    assert!(bound.completed_context_audit(&ids, || 70004).is_err());
    assert!(bound
        .completed_context_receipts(&ids, || panic!("revoked session reopened"))
        .is_err());
    drop(proof);
    drop(reserved);
    drop(receipt);
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn model_completion_deadline_is_independent_of_retrieval_session_time() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let binding = model_binding();
    let mut budget = model_budget();
    budget.max_duration_ms = 1;
    bound
        .configure_research_model(binding.clone(), budget, || 70003)
        .unwrap();
    let reserved = bound
        .reserve_research_context(&[], "", "task", 1, || 70003)
        .unwrap();
    bound
        .consume_model_context(
            &reserved.id,
            DispatchInput {
                invocation_id: "call",
                binding: &binding,
                system_prompt: &reserved.input.system_prompt,
                task: &reserved.input.task,
            },
            || 70003,
        )
        .unwrap();
    assert!(bound
        .complete_model_context(&reserved.id, "call", || 70004)
        .is_err());
    assert!(bound
        .completed_context_receipts(&[reserved.id], || 70004)
        .is_err());
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
