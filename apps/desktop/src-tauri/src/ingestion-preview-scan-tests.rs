use super::*;
use sha2::{Digest, Sha256};
use std::{io::Cursor, time::Duration};
fn limits() -> TelegramArchiveLimits {
    TelegramArchiveLimits {
        max_input_bytes: 100000,
        max_message_bytes: 8192,
        max_metadata_bytes: 1024,
        max_messages: 100,
        max_duration: Duration::from_secs(10),
    }
}
fn scan(input: &[u8]) -> Result<ScannedPreview, String> {
    scan_telegram_preview(
        Cursor::new(input),
        &limits(),
        100000,
        &AtomicBool::new(false),
    )
}
#[test]
fn preview_pins_complete_input_and_exact_resource_labels_with_known_authors() {
    let input=br#" {"name":"  Chat e\u0301  ","type":"personal_chat","id":-123,"messages":[{"id":1,"from_id":"user1","text":"A\r\nB"},{"id":2,"actor_id":"user2","type":"service"},{"id":3,"text":"unknown"}]} "#;
    let actual = scan(input).unwrap();
    assert_eq!(
        actual.fingerprint.sha256,
        format!("{:x}", Sha256::digest(input))
    );
    assert_eq!(actual.fingerprint.byte_length, input.len() as u64);
    assert_eq!(
        actual.resources,
        vec![PreviewResource {
            id: "telegram:peer:user:-123".into(),
            label: "  Chat é  ".into(),
            author_ids: vec!["user1".into(), "user2".into()],
            estimated_documents: Some(3)
        }]
    );
}
#[test]
fn empty_chats_are_real_resources_and_unknown_authors_are_not_fabricated() {
    let input=br#"{"about":"export","chats":{"about":"chats","list":[{"name":null,"type":null,"id":1,"messages":[]},{"type":"personal_chat","id":2,"messages":[{"from":"Label only"}]}]}}"#;
    let actual = scan(input).unwrap();
    assert_eq!(actual.resources.len(), 2);
    assert_eq!(actual.resources[0].estimated_documents, Some(0));
    assert_eq!(actual.resources[0].label, "");
    assert_eq!(actual.resources[1].estimated_documents, Some(1));
    assert!(actual.resources[1].author_ids.is_empty());
}
#[test]
fn never_returns_partial_preview_on_bad_tail_duplicate_chat_or_metadata_budget() {
    let base = br#"{"type":"personal_chat","id":1,"messages":[{"from_id":"user1"}]}"#;
    let mut bad = base.to_vec();
    bad.extend_from_slice(b" broken tail");
    assert!(scan(&bad).is_err());
    let duplicated = format!(
        r#"{{"about":"x","chats":{{"about":"x","list":[{0},{0}]}}}}"#,
        std::str::from_utf8(base).unwrap()
    );
    assert!(scan(duplicated.as_bytes()).is_err());
    assert!(
        scan_telegram_preview(Cursor::new(base), &limits(), 1, &AtomicBool::new(false)).is_err()
    );
}
#[test]
fn conflicting_or_malformed_author_identity_is_not_guessed() {
    for message in [
        r#"{"from_id":"one","actor_id":"two"}"#,
        r#"{"from_id":34}"#,
        r#"{"actor_id":"bad\nID"}"#,
    ] {
        let text = format!(r#"{{"type":"personal_chat","id":1,"messages":[{message}]}}"#);
        assert!(scan(text.as_bytes()).is_err());
    }
}
#[test]
fn bounded_streaming_counts_large_chat_without_retaining_bodies() {
    let body = "  точный текст é\r\n".repeat(100);
    let message = serde_json::json!({"from_id":"user1","text":body}).to_string();
    let source = format!(
        r#"{{"type":"personal_chat","id":1,"messages":[{}]}}"#,
        vec![message; 10400].join(",")
    );
    assert!(source.len() > 16 * 1024 * 1024);
    let mut limits = limits();
    limits.max_input_bytes = source.len() as u64;
    limits.max_messages = 10400;
    limits.max_duration = Duration::from_secs(60);
    let result = scan_telegram_preview(
        Cursor::new(source.as_bytes()),
        &limits,
        10000,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(result.resources[0].estimated_documents, Some(10400));
    assert_eq!(result.resources[0].author_ids, vec!["user1"]);
    assert_eq!(result.fingerprint.byte_length, source.len() as u64);
}

#[test]
fn duplicate_provider_author_keys_are_ambiguous_even_when_values_match() {
    for message in [
        r#"{"from_id":"one","from_id":"two"}"#,
        r#"{"actor_id":"one","actor_id":"one"}"#,
    ] {
        let text = format!(r#"{{"type":"personal_chat","id":1,"messages":[{message}]}}"#);
        assert!(scan(text.as_bytes()).is_err());
    }
}

#[test]
fn peer_class_keeps_same_bare_ids_separate_in_account_export_preview() {
    let input = br#"{"about":"export","chats":{"about":"chats","list":[{"name":"User","type":"personal_chat","id":42,"messages":[{"from_id":"user1"}]},{"name":"Group","type":"private_group","id":42,"messages":[{"from_id":"user2"}]},{"name":"Channel","type":"private_channel","id":42,"messages":[{"from_id":"channel42"}]}]}}"#;
    let preview = scan(input).unwrap();
    assert_eq!(preview.resources.len(), 3);
    let ids: std::collections::BTreeSet<_> = preview.resources.iter().map(|r| &r.id).collect();
    assert_eq!(ids.len(), 3);
    assert_eq!(preview.resources[0].author_ids, vec!["user1"]);
    assert_eq!(preview.resources[1].author_ids, vec!["user2"]);
    assert_eq!(preview.resources[2].author_ids, vec!["channel42"]);
}

#[test]
fn peer_class_legacy_preview_keeps_old_addresses_and_rejects_ambiguous_peers() {
    let input = br#"{"type":"personal_chat","id":42,"messages":[{"from_id":"user1"}]}"#;
    let legacy = scan_telegram_preview_version(
        ArchiveVersion::V1,
        input.as_slice(),
        &limits(),
        100000,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(legacy.resources[0].id, "telegram:chat:42");
    assert_eq!(legacy.resources[0].author_ids, vec!["user1"]);
    let ambiguous = br#"{"about":"export","chats":{"about":"chats","list":[{"type":"personal_chat","id":42,"messages":[]},{"type":"public_channel","id":42,"messages":[]}]}}"#;
    assert_eq!(scan(ambiguous).unwrap().resources.len(), 2);
    assert!(scan_telegram_preview_version(
        ArchiveVersion::V1,
        ambiguous.as_slice(),
        &limits(),
        100000,
        &AtomicBool::new(false),
    )
    .is_err());
}
