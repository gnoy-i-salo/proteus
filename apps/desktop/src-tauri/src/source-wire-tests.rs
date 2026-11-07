use super::model::*;
use crate::connections::Provider;
use serde_json::json;
#[test]
fn source_read_accepts_canonical_document_ids() {
    let action =
        serde_json::from_value::<SourceAction>(json!({"action":"read","documentIds":["d:1"]}));
    assert!(matches!(action, Ok(SourceAction::Read { document_ids }) if document_ids == ["d:1"]));
}
#[test]
fn source_read_rejects_legacy_wire() {
    assert!(serde_json::from_value::<SourceAction>(
        json!({"action":"read","document-ids":["d:1"]})
    )
    .is_err());
}
#[test]
fn source_budget_emits_exact_canonical_keys() {
    let value = serde_json::to_value(SourceBudget {
        max_steps: 1,
        max_documents: 2,
        max_bytes: 1024,
        max_duration_ms: 1000,
    })
    .unwrap();
    assert_eq!(
        value,
        json!({"maxSteps":1,"maxDocuments":2,"maxBytes":1024,"maxDurationMs":1000})
    );
}
#[test]
fn source_document_keeps_null_presence_and_exact_text() {
    let text = "\r\n  Чужой: é 😀\t";
    let value = serde_json::to_value(SourceDocument {
        id: "d1".into(),
        provider: Provider::Telegram,
        resource_id: "chat:1".into(),
        author_id: None,
        author_label: None,
        authorship: "unknown".into(),
        kind: "message".into(),
        mode: "dialogue".into(),
        text: text.into(),
        source_url: None,
        created_at: None,
        retrieved_at: None,
        content_hash: "h".into(),
        reply_to: None,
        is_forwarded: true,
        truncated: false,
        partition: "training".into(),
    })
    .unwrap();
    assert_eq!(
        value,
        json!({"id":"d1","provider":"telegram","resourceId":"chat:1","authorId":null,"authorLabel":null,
            "authorship":"unknown","kind":"message","mode":"dialogue","text":text,"sourceUrl":null,"createdAt":null,"retrievedAt":null,
            "contentHash":"h","replyTo":null,"isForwarded":true,"truncated":false,"partition":"training"})
    );
}
#[test]
fn matches_shared_frontend_wire_fixture() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../src/shared/lib/fixtures/native-wire.json"
    ))
    .unwrap();
    let value: SourceGrantRequest =
        serde_json::from_value(fixture["sourceRequest"].clone()).unwrap();
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        fixture["sourceRequest"]
    );
    let value: SourceGrant = serde_json::from_value(fixture["sourceGrant"].clone()).unwrap();
    assert_eq!(serde_json::to_value(value).unwrap(), fixture["sourceGrant"]);
    let value: SourceResult = serde_json::from_value(fixture["sourceResult"].clone()).unwrap();
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        fixture["sourceResult"]
    );
    let value: SourceAction = serde_json::from_value(fixture["sourceAction"].clone()).unwrap();
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        fixture["sourceAction"]
    );
    let value: ImportedSource = serde_json::from_value(fixture["importedSource"].clone()).unwrap();
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        fixture["importedSource"]
    );
}
