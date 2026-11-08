use super::*;
#[test]
fn native_assembly_preserves_original_prefix_and_hashes_the_actual_framed_input() {
    let prompt = "  Исходный prompt\r\n😀é\u{0000}";
    let task = "\r\n Точная задача ";
    let result = prepare(prompt, task, &[], &[]).unwrap();
    assert_eq!(result.system_prompt, prompt);
    assert!(result.task.starts_with(&format!("{task}{CONTEXT_MARKER}")));
    let context = &result.task[task.len() + CONTEXT_MARKER.len()..];
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(context).unwrap(),
        serde_json::json!({"schemaVersion":1,"retrievals":[],"items":[]})
    );
    assert_eq!(
        result.identity,
        model_input_identity(prompt, &result.task).unwrap()
    );
    assert_ne!(result.identity, model_input_identity(prompt, task).unwrap());
    assert_eq!(
        result.context_sha256,
        format!("{:x}", Sha256::digest(context.as_bytes()))
    );
}
#[test]
fn assembled_context_counts_against_existing_cli_limits_without_truncation() {
    let minimal = prepare("", "", &[], &[]).unwrap();
    let exact = "a".repeat(MAX_TASK_BYTES - minimal.task.len());
    assert_eq!(
        prepare(&"s".repeat(MAX_SYSTEM_BYTES), &exact, &[], &[])
            .unwrap()
            .task
            .len(),
        MAX_TASK_BYTES
    );
    assert!(prepare("", &(exact + "a"), &[], &[]).is_err());
    assert!(prepare(&"s".repeat(MAX_SYSTEM_BYTES + 1), "", &[], &[]).is_err());
    assert!(prepare("", &"a".repeat(MAX_TASK_BYTES + 1), &[], &[]).is_err());
}
