use super::*;
use serde_json::{json, Value};
const FIXTURE: &str =
    include_str!("../../../../packages/contracts/src/testing/persona-profile-snapshot.json");
fn value() -> Value {
    serde_json::from_str(FIXTURE).unwrap()
}
fn valid(value: Value) -> bool {
    serde_json::from_value::<PersonaProfileSnapshot>(value)
        .is_ok_and(|body| body.validate().is_ok())
}
#[test]
fn shared_profile_fixture_roundtrips_exact_source_text() {
    let body: PersonaProfileSnapshot = serde_json::from_str(FIXTURE).unwrap();
    body.validate().unwrap();
    assert_eq!(serde_json::to_value(&body).unwrap(), value());
    assert_eq!(body.archetype, "\u{feff}  Маша e\u{301} 👩🏽‍🔬\r\n\0дословно  ");
}
#[test]
fn insufficient_profile_does_not_invent_samples_or_traits() {
    let mut body = value();
    body["rules"] = json!([]);
    body["sourceStatus"] = json!("synthetic");
    body["styleExemplars"] = json!([]);
    body["styleProfile"]["voiceRules"] = json!([]);
    body["styleProfile"]["styleExemplarIds"] = json!([]);
    for mode in body["styleProfile"]["contextModes"].as_array_mut().unwrap() {
        mode["status"] = json!("insufficient");
    }
    assert!(valid(body));
}
#[test]
fn task_free_schema_rejects_unknown_fields_and_noncanonical_shapes() {
    for field in [
        "memory",
        "id",
        "taskSkills",
        "skills",
        "sourceContext",
        "source-status",
    ] {
        let mut body = value();
        body[field] = json!([]);
        assert!(!valid(body), "{field}");
    }
    for (pointer, replacement) in [
        ("/schemaVersion", json!(2)),
        ("/styleProfile/version", json!(1)),
        ("/palette", json!("green")),
        ("/sourceStatus", json!("neutral")),
        ("/rules/0/evidenceIds", json!([])),
        ("/rules/0/status", json!("conflicting")),
        ("/styleExemplars", json!([])),
        ("/styleProfile/voiceRules/0/exemplarIds", json!(["foreign"])),
        ("/styleProfile/contextModes/1/mode", json!("dialogue")),
        ("/styleProfile/contextModes/1/status", json!("observed")),
        ("/styleExemplars/0/exportAllowed", json!(true)),
        (
            "/styleExemplars/0/redactions",
            json!([{ "start":0, "end":1 }]),
        ),
    ] {
        let mut body = value();
        *body.pointer_mut(pointer).unwrap() = replacement;
        assert!(!valid(body), "{pointer}");
    }
}
#[test]
fn duplicate_ids_and_observed_reasoning_are_rejected() {
    for pointer in ["/rules", "/styleExemplars", "/styleProfile/voiceRules"] {
        let mut body = value();
        let list = body.pointer_mut(pointer).unwrap().as_array_mut().unwrap();
        list.push(list[0].clone());
        assert!(!valid(body), "{pointer}");
    }
    let mut body = value();
    body["styleProfile"]["reasoningPatterns"] = body["styleProfile"]["voiceRules"].clone();
    body["styleProfile"]["voiceRules"] = json!([]);
    assert!(!valid(body));
}
#[test]
fn complete_body_budget_counts_utf8_without_truncation() {
    let mut body = value();
    body["styleExemplars"][0]["context"] = json!("я".repeat(262144));
    assert!(!valid(body));
}

#[test]
fn explicit_neutral_profile_has_no_persona_traits_or_samples() {
    let mut body = value();
    body["rules"] = json!([]);
    body["sourceStatus"] = json!("neutral");
    body["styleExemplars"] = json!([]);
    body["styleProfile"]["voiceRules"] = json!([]);
    body["styleProfile"]["styleExemplarIds"] = json!([]);
    for mode in body["styleProfile"]["contextModes"].as_array_mut().unwrap() {
        mode["status"] = json!("insufficient");
    }
    assert!(valid(body));
}
