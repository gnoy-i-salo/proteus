use super::super::{DispatchInput, InvocationBinding, ModelContexts, ReservedContext};
fn running() -> (ModelContexts, ReservedContext) {
    let binding = InvocationBinding {
        provider: "codex".into(),
        model: "pinned".into(),
        effort: "high".into(),
        operation: "persona-reply".into(),
    };
    let mut contexts = ModelContexts::new(binding.clone(), 2, 100000, 100, 10).unwrap();
    let reserved = contexts
        .reserve("instructions", "exact task", vec![], 1, &|| Ok(11))
        .unwrap();
    contexts
        .consume(
            &reserved.id,
            DispatchInput {
                invocation_id: "call-1",
                binding: &binding,
                system_prompt: &reserved.input.system_prompt,
                task: &reserved.input.task,
            },
            &|| Ok(12),
        )
        .unwrap();
    (contexts, reserved)
}
#[test]
fn completed_output_binds_exact_unicode_whitespace_lease_and_invocation() {
    let (mut contexts, reserved) = running();
    let answer = "\u{feff}  Анна → Борис\r\n**да** e\u{301} 👩🏽‍🔬  ";
    assert!(contexts
        .check_completed_output(&reserved.id, "call-1", answer, &|| Ok(12))
        .is_err());
    contexts
        .complete_with_output(&reserved.id, "call-1", answer, &|| Ok(13))
        .unwrap();
    contexts
        .check_completed_output(&reserved.id, "call-1", answer, &|| Ok(14))
        .unwrap();
    for changed in [
        answer.trim().to_string(),
        answer.replace("\r\n", "\n"),
        answer.replace("e\u{301}", "é"),
        "fabricated answer".into(),
    ] {
        assert!(contexts
            .check_completed_output(&reserved.id, "call-1", &changed, &|| Ok(14))
            .is_err());
    }
    for (id, invocation) in [("foreign", "call-1"), (reserved.id.as_str(), "call-2")] {
        assert!(contexts
            .check_completed_output(id, invocation, answer, &|| Ok(14))
            .is_err());
    }
    assert!(contexts
        .complete_with_output(&reserved.id, "call-1", "replacement", &|| Ok(14))
        .is_err());
    contexts
        .check_completed_output(&reserved.id, "call-1", answer, &|| Ok(15))
        .unwrap();
    let audit = contexts
        .completed_audit(&[reserved.id], &|| Ok(15))
        .unwrap();
    assert_eq!(audit[0].schema_version, 1);
    assert!(!serde_json::to_string(&audit).unwrap().contains("Анна"));
}
#[test]
fn malformed_output_never_completes_and_completion_without_output_is_not_output_proof() {
    let (mut contexts, reserved) = running();
    for answer in ["".into(), " \r\n\t".into(), "x".repeat(4 * 1024 * 1024 + 1)] {
        assert!(contexts
            .complete_with_output(&reserved.id, "call-1", &answer, &|| Ok(13))
            .is_err());
        assert!(contexts
            .completed_audit(std::slice::from_ref(&reserved.id), &|| Ok(13))
            .is_err());
    }
    contexts
        .complete_with_output(&reserved.id, "call-1", "valid", &|| Ok(13))
        .unwrap();
    contexts
        .check_completed_output(&reserved.id, "call-1", "valid", &|| Ok(14))
        .unwrap();
    let (mut legacy, reserved) = running();
    legacy.complete(&reserved.id, "call-1", &|| Ok(13)).unwrap();
    assert!(legacy
        .check_completed_output(&reserved.id, "call-1", "caller-invented", &|| Ok(14))
        .is_err());
}
#[test]
fn expired_or_revoked_output_proof_never_reopens() {
    for revoked in [true, false] {
        let (mut contexts, reserved) = running();
        contexts
            .complete_with_output(&reserved.id, "call-1", "native answer", &|| Ok(13))
            .unwrap();
        assert!(contexts
            .check_completed_output(&reserved.id, "call-1", "native answer", &|| if revoked {
                Err("revoked".into())
            } else {
                Ok(110)
            })
            .is_err());
        assert!(contexts
            .check_completed_output(&reserved.id, "call-1", "native answer", &|| panic!(
                "closed proof queried clock"
            ))
            .is_err());
    }
}
