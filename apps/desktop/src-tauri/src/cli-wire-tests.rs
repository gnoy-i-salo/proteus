use super::*;
use serde_json::json;
#[test]
fn cli_request_accepts_canonical_owned_fields() {
    let request = serde_json::from_value::<Request>(
        json!({"id":"r1","provider":"codex","model":"m","task":"  \r\n😀", "systemPrompt":"  system\t","effort":"high","sourceContext":true}),
    );
    assert!(
        matches!(request,Ok(request) if request.source_context && request.system_prompt == "  system\t" && request.operation == "generation")
    );
}
#[test]
fn cli_events_use_canonical_elapsed_ms() {
    let event = serde_json::to_value(Event {
        id: "r1".into(),
        kind: "started".into(),
        elapsed_ms: 17,
        reasoning: None,
    })
    .unwrap();
    assert_eq!(event, json!({"id":"r1","kind":"started","elapsedMs":17}));
}
#[test]
fn matches_shared_frontend_wire_fixture() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../src/shared/lib/fixtures/native-wire.json"
    ))
    .unwrap();
    let value: Request = serde_json::from_value(fixture["cliRequest"].clone()).unwrap();
    assert_eq!(serde_json::to_value(value).unwrap(), fixture["cliRequest"]);
    let value: Response = serde_json::from_value(fixture["cliResult"].clone()).unwrap();
    assert_eq!(serde_json::to_value(value).unwrap(), fixture["cliResult"]);
    let value: Event = serde_json::from_value(fixture["cliEvent"].clone()).unwrap();
    assert_eq!(serde_json::to_value(value).unwrap(), fixture["cliEvent"]);
}
