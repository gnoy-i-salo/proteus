use super::*;
fn request(answer: &str) -> AppendSimulationRequest {
    AppendSimulationRequest {
        schema_version: 1,
        experiment_id: "experiment:1".into(),
        trial_id: "trial:1".into(),
        context_lease_id: "modelContext:1".into(),
        invocation_id: "call-1".into(),
        answer: answer.into(),
    }
}
#[test]
fn simulation_wire_preserves_reply_messages_and_accepts_only_host_scope() {
    let messages = ["  Анна → Борис\r\n**да** e\u{301} 👩🏽‍🔬  ", "\tещё\n\n"];
    let json = serde_json::json!({"action":"reply","messages":messages,"summary":" Готово ","styleConflicts":[]}).to_string();
    for answer in [
        json.clone(),
        format!("\u{feff} \n```json\r\n{json}\n``` \u{a0}"),
    ] {
        let input = request(&answer);
        input.validate().unwrap();
        assert_eq!(input.reply_text().unwrap(), messages.join("\n\n"));
        let encoded = serde_json::to_value(&input).unwrap();
        assert_eq!(encoded["answer"], answer);
        for key in [
            "manifestId",
            "role",
            "id",
            "createdAt",
            "experiment-id",
            "context_lease_id",
            "unknown",
        ] {
            let mut invalid = encoded.clone();
            invalid[key] = "forged".into();
            assert!(serde_json::from_value::<AppendSimulationRequest>(invalid).is_err());
        }
    }
}
#[test]
fn simulation_wire_rejects_tool_outputs_scope_overrides_and_oversized_dialogue() {
    for answer in [
        "null".into(), "[]".into(), "{\"action\":\"recall\",\"query\":\"x\",\"intent\":\"facts\"}".into(),
        serde_json::json!({"action":"reply","messages":["ok"],"summary":"done","styleConflicts":[],"role":"user"}).to_string(),
        serde_json::json!({"action":"reply","messages":[],"summary":"done","styleConflicts":[]}).to_string(),
        serde_json::json!({"action":"reply","messages":[" "],"summary":"done","styleConflicts":[]}).to_string(),
        serde_json::json!({"action":"reply","messages":["x".repeat(65536),"x"],"summary":"done","styleConflicts":[]}).to_string(),
    ] { assert!(request(&answer).reply_text().is_err()); }
    let valid = serde_json::json!({"action":"reply","messages":["x".repeat(65536)],"summary":"done","styleConflicts":[]}).to_string();
    assert_eq!(request(&valid).reply_text().unwrap().len(), 65536);
    for field in [
        "schemaVersion",
        "experimentId",
        "trialId",
        "contextLeaseId",
        "invocationId",
        "answer",
    ] {
        let mut encoded = serde_json::to_value(request(&valid)).unwrap();
        encoded[field] = if field == "schemaVersion" {
            serde_json::json!(2)
        } else {
            serde_json::json!("\n")
        };
        let invalid: AppendSimulationRequest = serde_json::from_value(encoded).unwrap();
        assert!(invalid.validate().is_err(), "{field}");
    }
}
