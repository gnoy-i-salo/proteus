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
