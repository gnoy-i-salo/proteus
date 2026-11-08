use super::*;
fn binding() -> InvocationBinding {
    InvocationBinding {
        provider: "codex".into(),
        model: "chosen-model".into(),
        effort: "medium".into(),
        operation: "corpus-analysis".into(),
    }
}
fn dispatch<'a>(
    binding: &'a InvocationBinding,
    input: &'a PreparedModelInput,
    id: &'a str,
) -> DispatchInput<'a> {
    DispatchInput {
        invocation_id: id,
        binding,
        system_prompt: &input.system_prompt,
        task: &input.task,
    }
}
#[test]
fn exact_input_and_model_binding_are_single_use_and_completion_is_required() {
    let binding = binding();
    let mut contexts = ModelContexts::new(binding.clone(), 2, 100000, 100, 10).unwrap();
    let reserved = contexts
        .reserve("  prompt\r\n", "задача", vec![], 1, &|| Ok(11))
        .unwrap();
    let ids = [reserved.id.clone()];
    assert_eq!(reserved.usage.used_calls, 1);
    assert!(contexts
        .completed_receipts(&ids, &|| panic!("unused context checked grant"))
        .is_err());
    assert!(contexts
        .complete(&reserved.id, "call-1", &|| panic!(
            "unused context completed"
        ))
        .is_err());
    let mut changed = binding.clone();
    changed.model = "other-model".into();
    assert!(contexts
        .consume(
            &reserved.id,
            dispatch(&changed, &reserved.input, "call-1"),
            &|| panic!("wrong model checked grant")
        )
        .is_err());
    assert!(contexts
        .consume(
            &reserved.id,
            DispatchInput {
                invocation_id: "call-1",
                binding: &binding,
                system_prompt: "changed",
                task: &reserved.input.task
            },
            &|| panic!("wrong input checked grant")
        )
        .is_err());
    contexts
        .consume(
            &reserved.id,
            dispatch(&binding, &reserved.input, "call-1"),
            &|| Ok(12),
        )
        .unwrap();
    assert!(contexts
        .consume(
            &reserved.id,
            dispatch(&binding, &reserved.input, "call-1"),
            &|| panic!("reused context checked grant")
        )
        .is_err());
    assert!(contexts
        .completed_receipts(&ids, &|| panic!("incomplete context checked grant"))
        .is_err());
    assert!(contexts
        .complete(&reserved.id, "different-call", &|| panic!(
            "wrong call completed"
        ))
        .is_err());
    contexts
        .complete(&reserved.id, "call-1", &|| Ok(13))
        .unwrap();
    assert!(contexts
        .completed_receipts(&ids, &|| Ok(14))
        .unwrap()
        .is_empty());
    assert!(contexts
        .completed_receipts(&[ids[0].clone(), ids[0].clone()], &|| panic!(
            "duplicate completion checked grant"
        ))
        .is_err());
    let repair = contexts
        .reserve("prompt", "repair", vec![], 2, &|| Ok(15))
        .unwrap();
    assert_eq!(repair.usage.used_calls, 2);
    assert!(contexts
        .reserve("", "third", vec![], 1, &|| panic!(
            "exhausted budget checked grant"
        ))
        .is_err());
    assert!(contexts
        .completed_receipts(&ids, &|| Ok(15))
        .unwrap()
        .is_empty());
    assert!(contexts
        .consume(
            &repair.id,
            dispatch(&binding, &repair.input, "call-1"),
            &|| panic!("invocation id reused")
        )
        .is_err());
}

fn empty_receipt(id: &str) -> Arc<RecordedRetrieval> {
    Arc::new(RecordedRetrieval {
        receipt: crate::memory::session::RetrievalReceipt {
            schema_version: 2,
            operation: "recall",
            id: id.into(),
            snapshot_id: "snapshot".into(),
            policy_version: "policy".into(),
            retrieval_version: "nativeMemoryRetrieval:1",
            index_generation: 1,
            embedding_fingerprint: None,
            mode: "lexical",
            hits: vec![],
            elapsed_ms: 0,
            source_bytes: 0,
            context_tokens: 0,
        },
        sources: vec![],
    })
}

#[test]
fn completed_contexts_deduplicate_receipts_and_recheck_before_proof_exposure() {
    let binding = binding();
    let mut contexts = ModelContexts::new(binding.clone(), 2, 100000, 100, 10).unwrap();
    let a = empty_receipt("a");
    let b = empty_receipt("b");
    let first = contexts
        .reserve("", "first", vec![a.clone()], 1, &|| Ok(11))
        .unwrap();
    let second = contexts
        .reserve("", "second", vec![a, b], 2, &|| Ok(11))
        .unwrap();
    contexts
        .consume(
            &first.id,
            dispatch(&binding, &first.input, "first"),
            &|| Ok(12),
        )
        .unwrap();
    contexts
        .consume(
            &second.id,
            dispatch(&binding, &second.input, "second"),
            &|| Ok(12),
        )
        .unwrap();
    contexts.complete(&first.id, "first", &|| Ok(13)).unwrap();
    contexts.complete(&second.id, "second", &|| Ok(13)).unwrap();
    let ids = [second.id, first.id];
    let calls = std::cell::Cell::new(0);
    let proof = contexts
        .completed_receipts(&ids, &|| {
            calls.set(calls.get() + 1);
            Ok(14)
        })
        .unwrap();
    assert_eq!(
        proof
            .iter()
            .map(|r| r.receipt.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    let final_call = calls.get();
    calls.set(0);
    assert!(contexts
        .completed_receipts(&ids, &|| {
            calls.set(calls.get() + 1);
            if calls.get() == final_call {
                Err("late revocation".into())
            } else {
                Ok(14)
            }
        })
        .is_err());
    assert!(contexts
        .completed_receipts(&ids, &|| panic!("closed proof reopened"))
        .is_err());
}

#[test]
fn late_completion_failure_never_exposes_even_an_earlier_successful_context() {
    let binding = binding();
    let mut contexts = ModelContexts::new(binding.clone(), 2, 100000, 100, 10).unwrap();
    let first = contexts
        .reserve("", "first", vec![], 1, &|| Ok(11))
        .unwrap();
    let second = contexts
        .reserve("", "second", vec![], 2, &|| Ok(11))
        .unwrap();
    contexts
        .consume(
            &first.id,
            dispatch(&binding, &first.input, "first"),
            &|| Ok(12),
        )
        .unwrap();
    contexts
        .consume(
            &second.id,
            dispatch(&binding, &second.input, "second"),
            &|| Ok(12),
        )
        .unwrap();
    let calls = std::cell::Cell::new(0);
    contexts
        .complete(&first.id, "first", &|| {
            calls.set(calls.get() + 1);
            Ok(13)
        })
        .unwrap();
    let final_call = calls.get();
    calls.set(0);
    assert!(contexts
        .complete(&second.id, "second", &|| {
            calls.set(calls.get() + 1);
            if calls.get() == final_call {
                Err("late revocation".into())
            } else {
                Ok(13)
            }
        })
        .is_err());
    for id in [first.id, second.id] {
        assert!(contexts
            .completed_receipts(&[id], &|| panic!("failed completion reopened"))
            .is_err());
    }
}
#[test]
fn revoked_consumption_and_expired_completion_never_produce_analysis_proof() {
    let binding = binding();
    for revoke_before_consume in [true, false] {
        let mut contexts = ModelContexts::new(binding.clone(), 2, 100000, 100, 10).unwrap();
        let reserved = contexts.reserve("", "task", vec![], 1, &|| Ok(11)).unwrap();
        if revoke_before_consume {
            assert!(contexts
                .consume(
                    &reserved.id,
                    dispatch(&binding, &reserved.input, "call-1"),
                    &|| Err("grant revoked".into())
                )
                .is_err());
        } else {
            contexts
                .consume(
                    &reserved.id,
                    dispatch(&binding, &reserved.input, "call-1"),
                    &|| Ok(12),
                )
                .unwrap();
            assert!(contexts
                .complete(&reserved.id, "call-1", &|| Ok(110))
                .is_err());
        }
        assert!(contexts
            .completed_receipts(&[reserved.id], &|| panic!("invalid proof checked grant"))
            .is_err());
    }
}
