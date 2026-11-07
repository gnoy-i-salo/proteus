use super::*;
use crate::ingestion::telegram_archive_json::TelegramChatHeader;
fn frame(raw: &str) -> TelegramMessageFrame {
    TelegramMessageFrame {
        chat: TelegramChatHeader {
            id: "42".into(),
            name: Some("  Диалог é\r\n".into()),
            kind: "personal_chat".into(),
        },
        raw_json: raw.as_bytes().to_vec(),
        byte_start: 100,
        byte_end: 100 + raw.len() as u64,
    }
}
fn origin() -> String {
    format!("localArchivePath:1:sha256:{}", "a".repeat(64))
}
#[test]
fn exact_original_and_rich_text_survive_without_invented_authorship_or_acquisition_time() {
    let raw = r#"{ "id":7,"type":"message","from":"  Маша é ","from_id":"user7","date_unixtime":"1704067200","edited_unixtime":"1704067260","text":["  Привет\r\n",{"type":"bold","text":"é 😀"},"  "],"reply_to_message_id":6,"forwarded_from":"  Друг ","photo":"photos/a.jpg","custom":{"External_Key":"UNCHANGED"} }"#;
    let value = normalize(&origin(), frame(raw)).unwrap();
    assert_eq!(value.schema_version, 1);
    assert_eq!(value.raw_json, raw);
    assert_eq!(value.text, "  Привет\r\né 😀  ");
    assert_eq!(value.author_label.as_deref(), Some("  Маша é "));
    assert_eq!(value.author_id.as_deref(), Some("user7"));
    assert_eq!(value.resource_label.as_deref(), Some("  Диалог é\r\n"));
    assert_eq!(value.created_at.as_deref(), Some("2024-01-01T00:00:00Z"));
    assert_eq!(value.edited_at.as_deref(), Some("2024-01-01T00:01:00Z"));
    assert_eq!(value.reply_to_original_id.as_deref(), Some("6"));
    assert!(value.is_forwarded);
    let encoded = serde_json::to_value(&value).unwrap();
    assert!(encoded.get("schemaVersion").is_some());
    assert!(encoded.get("rawJson").is_some());
    assert!(encoded.get("author_id").is_none());
    assert!(encoded.get("grantId").is_none());
    assert!(encoded.get("retrievedAt").is_none());
}
#[test]
fn stable_identity_distinguishes_messages_origins_and_resources_but_survives_edits() {
    let a = normalize(&origin(), frame(r#"{"id":1,"type":"message","text":"ok"}"#)).unwrap();
    let b = normalize(&origin(), frame(r#"{"id":2,"type":"message","text":"ok"}"#)).unwrap();
    let edited = normalize(
        &origin(),
        frame(r#"{"id":1,"type":"message","text":"edit"}"#),
    )
    .unwrap();
    let other = normalize(
        &format!("localArchivePath:1:sha256:{}", "b".repeat(64)),
        frame(r#"{"id":1,"type":"message","text":"ok"}"#),
    )
    .unwrap();
    let mut different_chat = frame(r#"{"id":1,"type":"message","text":"ok"}"#);
    different_chat.chat.id = "43".into();
    let different_chat = normalize(&origin(), different_chat).unwrap();
    assert_eq!(a.id, edited.id);
    assert_eq!(a.source_key, edited.source_key);
    assert_ne!(a.raw_json, edited.raw_json);
    assert_ne!(a.id, b.id);
    assert_ne!(a.id, other.id);
    assert_ne!(a.id, different_chat.id);
}
#[test]
fn service_actor_and_missing_author_are_explicit_and_large_integer_ids_stay_exact() {
    let event = normalize(&origin(), frame(r#"{"id":9007199254740993,"type":"service","actor":"Аня","actor_id":"user9","action":"pin_message","message_id":3}"#)).unwrap();
    assert_eq!(event.original_id, "9007199254740993");
    assert_eq!(event.kind, "service");
    assert_eq!(event.author_id.as_deref(), Some("user9"));
    assert!(event.text.is_empty());
    let unknown = normalize(
        &origin(),
        frame(r#"{"id":1,"type":"message","date":"2024-01-01T00:00:00","text":" ","from":"Имя"}"#),
    )
    .unwrap();
    assert!(unknown.author_id.is_none());
    assert!(
        unknown.created_at.is_none(),
        "timezone-free display date is not an absolute time"
    );
    assert_eq!(unknown.author_label.as_deref(), Some("Имя"));
}
#[test]
fn malformed_identity_authors_text_or_timestamps_never_silently_drop_source_content() {
    for raw in [
        r#"{"type":"message","text":"ok"}"#,
        r#"{"id":1,"id":2,"type":"message","text":"ok"}"#,
        r#"{"id":1.5,"type":"message","text":"ok"}"#,
        r#"{"id":1,"type":"future","text":"ok"}"#,
        r#"{"id":1,"type":"message","text":["ok",4]}"#,
        r#"{"id":1,"type":"message","text":[{"type":"bold"}]}"#,
        r#"{"id":1,"type":"message","text":"ok","from_id":"user1","actor_id":"user2"}"#,
        r#"{"id":1,"type":"message","text":"ok","date_unixtime":"broken"}"#,
        r#"{"id":1,"type":"message","text":"ok","reply_to_message_id":2.5}"#,
    ] {
        assert!(normalize(&origin(), frame(raw)).is_err(), "{raw}");
    }
    let mut bad = frame(r#"{"id":1,"type":"message","text":"ok"}"#);
    bad.byte_end -= 1;
    assert!(normalize(&origin(), bad).is_err());
    assert!(normalize(
        "/tmp/not-a-capability",
        frame(r#"{"id":1,"type":"message","text":"ok"}"#)
    )
    .is_err());
}
