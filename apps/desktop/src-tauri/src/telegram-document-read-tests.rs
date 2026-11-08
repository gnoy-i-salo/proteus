use super::*;
use crate::corpus_database::{
    Acquisition, HashAlgorithm, ProvenanceLimitation, RevisionReference, SourceProvenance,
};
use crate::ingestion::{
    telegram_archive_json::{TelegramChatHeader, TelegramMessageFrame},
    telegram_document::normalize_with_version,
};
fn origin() -> String {
    format!("localArchivePath:1:sha256:{}", "a".repeat(64))
}
fn stored(version: ArchiveVersion) -> StoredRevision {
    let raw = r#"{ "id":7,"type":"message","from_id":"user1","from":"  Аня é ","date_unixtime":"1704067200","text":["  A\r\n",{"type":"bold","text":"é 😀"}],"reply_to_message_id":6,"Extra_Field":{"Original_Key":"exact"} }"#;
    let doc = normalize_with_version(
        version,
        &origin(),
        TelegramMessageFrame {
            chat: TelegramChatHeader {
                id: "42".into(),
                name: Some("  Чат  ".into()),
                kind: "personal_chat".into(),
            },
            raw_json: raw.as_bytes().to_vec(),
            byte_start: 0,
            byte_end: raw.len() as u64,
        },
    )
    .unwrap();
    StoredRevision {
        reference: RevisionReference {
            document_id: doc.id.clone(),
            revision_id: "checked-by-database".into(),
        },
        source_key: doc.source_key.clone(),
        document_json: serde_json::to_string(&doc).unwrap(),
        provenance: SourceProvenance {
            acquisition: Acquisition::Archive,
            adapter_version: version.adapter().into(),
            source_format: "telegramDesktopJson".into(),
            original_object_id: Some("7".into()),
            original_byte_length: Some(raw.len() as u64),
            hash_algorithm: HashAlgorithm::Sha256,
            limitations: vec![
                ProvenanceLimitation::AttachmentsUnavailable,
                ProvenanceLimitation::RelationsIncomplete,
            ],
        },
    }
}
#[test]
fn stored_projection_reads_both_frozen_generations_without_mutating_originals() {
    for version in [ArchiveVersion::V1, ArchiveVersion::V2] {
        let revision = stored(version);
        let before = revision.clone();
        let doc = decode_revision(version, &origin(), &revision).unwrap();
        assert_eq!(doc.text, "  A\r\né 😀");
        assert_eq!(doc.author_label.as_deref(), Some("  Аня é "));
        assert_eq!(doc.resource_label.as_deref(), Some("  Чат  "));
        assert!(doc.raw_json.contains("Extra_Field"));
        assert_eq!(revision, before);
    }
}
#[test]
fn stored_projection_rejects_fields_that_disagree_with_exact_original() {
    for (field, replacement) in [
        ("text", serde_json::json!("edited")),
        ("authorId", serde_json::json!("user9")),
        ("authorLabel", serde_json::json!("renamed")),
        ("originalId", serde_json::json!("8")),
        ("createdAt", serde_json::json!("2025-01-01T00:00:00Z")),
        ("replyToOriginalId", serde_json::Value::Null),
        ("isForwarded", serde_json::json!(true)),
        ("kind", serde_json::json!("service")),
        ("sourceKey", serde_json::json!("forged")),
        ("id", serde_json::json!("forged")),
        ("resourceId", serde_json::json!("telegram:peer:channel:42")),
        ("schemaVersion", serde_json::json!(3)),
    ] {
        let mut revision = stored(ArchiveVersion::V2);
        let mut value: serde_json::Value = serde_json::from_str(&revision.document_json).unwrap();
        value[field] = replacement;
        revision.document_json = value.to_string();
        assert!(
            decode_revision(ArchiveVersion::V2, &origin(), &revision).is_err(),
            "{field}"
        );
    }
}
#[test]
fn stored_projection_rejects_wrong_origin_version_reference_and_provenance() {
    let revision = stored(ArchiveVersion::V2);
    assert!(decode_revision(ArchiveVersion::V1, &origin(), &revision).is_err());
    let other_origin = format!("localArchivePath:1:sha256:{}", "b".repeat(64));
    assert!(decode_revision(ArchiveVersion::V2, &other_origin, &revision).is_err());
    for case in 0..7 {
        let mut changed = revision.clone();
        match case {
            0 => changed.source_key = "other".into(),
            1 => changed.reference.document_id = "other".into(),
            2 => changed.provenance.adapter_version = ArchiveVersion::V1.adapter().into(),
            3 => changed.provenance.acquisition = Acquisition::Api,
            4 => changed.provenance.source_format = "other".into(),
            5 => changed.provenance.original_object_id = Some("8".into()),
            _ => changed.provenance.original_byte_length = Some(1),
        }
        assert!(
            decode_revision(ArchiveVersion::V2, &origin(), &changed).is_err(),
            "{case}"
        );
    }
}
#[test]
fn stored_projection_rejects_missing_nullable_fields_and_unknown_owned_fields() {
    for field in [
        "authorLabel",
        "editedAt",
        "replyToOriginalId",
        "resourceLabel",
    ] {
        let mut revision = stored(ArchiveVersion::V2);
        let mut value: serde_json::Value = serde_json::from_str(&revision.document_json).unwrap();
        value.as_object_mut().unwrap().remove(field);
        revision.document_json = value.to_string();
        assert!(
            decode_revision(ArchiveVersion::V2, &origin(), &revision).is_err(),
            "{field}"
        );
    }
    let mut revision = stored(ArchiveVersion::V2);
    let mut value: serde_json::Value = serde_json::from_str(&revision.document_json).unwrap();
    value["author_id"] = serde_json::json!("user1");
    revision.document_json = value.to_string();
    assert!(decode_revision(ArchiveVersion::V2, &origin(), &revision).is_err());
}

#[test]
fn stored_projection_accepts_literal_v1_envelope_without_using_current_writer() {
    let mut revision = stored(ArchiveVersion::V1);
    revision.document_json = r#"{"schemaVersion":1,"id":"document:45daa8e37e1c005b01104aa5044419b2efc6518ec53de3c74e84f5d29f189b3e","sourceKey":"telegramArchive:1:45daa8e37e1c005b01104aa5044419b2efc6518ec53de3c74e84f5d29f189b3e","originalId":"7","resourceId":"telegram:chat:42","resourceLabel":null,"resourceKind":"personal_chat","kind":"message","authorId":null,"authorLabel":null,"text":"  old original  ","createdAt":null,"editedAt":null,"replyToOriginalId":null,"isForwarded":false,"rawJson":"{\"id\":7,\"type\":\"message\",\"text\":\"  old original  \"}"}"#.into();
    revision.provenance.original_byte_length =
        Some(r#"{"id":7,"type":"message","text":"  old original  "}"#.len() as u64);
    let before = revision.clone();
    let decoded = decode_revision(ArchiveVersion::V1, &origin(), &revision).unwrap();
    assert_eq!(decoded.text, "  old original  ");
    assert_eq!(decoded.author_id, None);
    assert_eq!(revision, before);
}
#[test]
fn stored_projection_rejects_oversize_duplicate_owned_keys_and_false_completeness() {
    let mut revision = stored(ArchiveVersion::V2);
    let original = revision.document_json.clone();
    revision.document_json = format!("{{\"schemaVersion\":2,{}", &original[1..]);
    assert!(decode_revision(ArchiveVersion::V2, &origin(), &revision).is_err());
    revision.document_json = original;
    revision.provenance.limitations.clear();
    assert!(decode_revision(ArchiveVersion::V2, &origin(), &revision).is_err());
    revision.document_json = " ".repeat(32 * 1024 * 1024 + 1);
    assert!(decode_revision(ArchiveVersion::V2, &origin(), &revision).is_err());
}
