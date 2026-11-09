use super::*;
use serde_json::{json, Value};
fn request() -> Value {
    json!({"schemaVersion":2,"purpose":"research","manifestId":null,
        "systemPrompt":"  Инструкция\r\n👩🏽‍🔬","task":"\u{feff}Задача\r\n  ",
        "receiptIds":["retrieval:1"],"memoryItemIds":[],"formatAttempt":1})
}
fn valid(value: Value) -> bool {
    serde_json::from_value::<ModelContextRequest>(value).is_ok_and(|r| r.validate().is_ok())
}
#[test]
fn exact_request_round_trip_and_required_fields() {
    let value = request();
    let decoded: ModelContextRequest = serde_json::from_value(value.clone()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), value);
    for key in value.as_object().unwrap().keys() {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(!valid(missing), "missing {key}");
    }
}
#[test]
fn rejects_untrusted_versions_hashes_scope_and_duplicate_ids() {
    for patch in [
        json!({"schemaVersion":1}),
        json!({"schemaVersion":3}),
        json!({"inputSha256":"a".repeat(64)}),
        json!({"inputBytes":10}),
        json!({"formatAttempt":0}),
        json!({"formatAttempt":3}),
        json!({"purpose":"arbitrary"}),
        json!({"manifestId":"foreign"}),
        json!({"memoryItemIds":["item:1"]}),
        json!({"receiptIds":["r:1","r:1"]}),
        json!({"receiptIds":[""]}),
        json!({"receiptIds":["id\n"]}),
        json!({"receiptIds":["я".repeat(129)]}),
        json!({"receiptIds":(0..1001).map(|i|format!("r:{i}")).collect::<Vec<_>>()}),
    ] {
        let mut value = request();
        value
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(!valid(value), "accepted {patch}");
    }
}
#[test]
fn validates_utf8_limits_and_persona_manifest_without_normalization() {
    let mut value = request();
    value["systemPrompt"] = json!("я".repeat(20000));
    value["task"] = json!("😀".repeat(50000));
    assert!(valid(value.clone()));
    value["systemPrompt"] = json!("я".repeat(20001));
    assert!(!valid(value.clone()));
    value["systemPrompt"] = json!("");
    value["task"] = json!("😀".repeat(50001));
    assert!(!valid(value));
    let mut persona = request();
    persona["purpose"] = json!("personaReply");
    assert!(!valid(persona.clone()));
    persona["manifestId"] = json!("manifest:1");
    persona["memoryItemIds"] = json!(["item:1"]);
    assert!(valid(persona.clone()));
    persona["memoryItemIds"] = json!(["item:1", "item:1"]);
    assert!(!valid(persona));
}
