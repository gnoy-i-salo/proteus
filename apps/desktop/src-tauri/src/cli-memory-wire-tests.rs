use super::*;
use serde_json::{json, Value};
fn memory_request() -> Value {
    json!({"id":"run-memory","provider":"codex","model":"fixture-model","effort":"medium",
        "task":"  Исходный контекст\r\n😀","systemPrompt":"  prompt\t","sourceContext":true,"operation":"corpus-analysis",
        "memoryContext":{"sessionId":"memorySession:opaque-id","leaseId":"modelContext:opaque-id"}})
}
#[test]
fn native_cli_memory_reference_preserves_exact_input_and_both_handles() {
    for operation in ["corpus-analysis", "persona-reply", "generation"] {
        let mut value = memory_request();
        value["operation"] = operation.into();
        let request: Request = serde_json::from_value(value.clone()).unwrap();
        validate(&request).unwrap();
        assert_eq!(serde_json::to_value(request).unwrap(), value);
    }
}
#[test]
fn native_cli_memory_reference_rejects_malformed_scope_extensions_and_aliases() {
    for context in [
        Value::Null,
        json!({}),
        json!({"sessionId":"s"}),
        json!({"sessionId":"","leaseId":"l"}),
        json!({"sessionId":"s","leaseId":"a".repeat(129)}),
        json!({"sessionId":" s ","leaseId":"l"}),
        json!({"sessionId":"s","leaseId":"l\n"}),
        json!({"sessionId":"s","leaseId":"l","personaId":"foreign"}),
        json!({"sessionId":"s","leaseId":"l","session_id":"legacy"}),
    ] {
        let mut value = memory_request();
        value["memoryContext"] = context;
        assert!(serde_json::from_value::<Request>(value)
            .map_err(|e| e.to_string())
            .and_then(|r| validate(&r))
            .is_err());
    }
    for (key, replacement) in [
        ("sourceContext", json!(false)),
        ("operation", json!("fabrication")),
        ("operation", json!("public-research")),
        ("operation", json!("source-plan")),
        ("provider", json!("kimi")),
    ] {
        let mut value = memory_request();
        value[key] = replacement;
        assert!(serde_json::from_value::<Request>(value)
            .map_err(|e| e.to_string())
            .and_then(|r| validate(&r))
            .is_err());
    }
}
