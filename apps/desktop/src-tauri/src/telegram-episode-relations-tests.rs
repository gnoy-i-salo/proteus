use super::*;
use crate::ingestion::{
    telegram_archive_json::{TelegramChatHeader, TelegramMessageFrame},
    telegram_document::normalize_with_version,
};
fn origin() -> String {
    format!("localArchivePath:1:sha256:{}", "a".repeat(64))
}
fn document(
    version: ArchiveVersion,
    peer: &str,
    body: serde_json::Value,
) -> (RevisionReference, TelegramDocument) {
    let raw = body.to_string();
    let doc = normalize_with_version(
        version,
        &origin(),
        TelegramMessageFrame {
            chat: TelegramChatHeader {
                id: "42".into(),
                name: Some(peer.into()),
                kind: peer.into(),
            },
            raw_json: raw.as_bytes().to_vec(),
            byte_start: 0,
            byte_end: raw.len() as u64,
        },
    )
    .unwrap();
    let reference = RevisionReference {
        document_id: doc.id.clone(),
        revision_id: format!("revision:{}", doc.id),
    };
    (reference, doc)
}
#[test]
fn relation_projection_links_same_chat_and_typed_cross_peer_replies() {
    let v = ArchiveVersion::V2;
    let docs = vec![
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":1,"type":"message","text":"user original"}),
        ),
        document(
            v,
            "private_channel",
            serde_json::json!({"id":1,"type":"message","text":"channel original"}),
        ),
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":2,"type":"message","text":"same chat","reply_to_message_id":1}),
        ),
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":3,"type":"message","text":"cross peer","reply_to_message_id":1,"reply_to_peer_id":"channel42"}),
        ),
    ];
    let edges = resolve(v, &origin(), &docs).unwrap();
    assert!(edges.iter().all(|r| r.complete));
    assert_eq!(edges[2].related, vec![docs[0].0.clone()]);
    assert_eq!(edges[3].related, vec![docs[1].0.clone()]);
}
#[test]
fn relation_projection_missing_reply_or_forward_never_claims_complete_history() {
    let v = ArchiveVersion::V2;
    let docs = vec![
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":1,"type":"message","text":"missing","reply_to_message_id":99}),
        ),
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":2,"type":"message","text":"forward","forwarded_from":"Original name","forwarded_from_id":"user7"}),
        ),
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":3,"type":"message","text":"future","unknown_relation":{"id":99}}),
        ),
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":4,"type":"message","text":"invalid namespace","reply_to_message_id":1,"reply_to_peer_id":"something42"}),
        ),
    ];
    assert!(resolve(v, &origin(), &docs)
        .unwrap()
        .iter()
        .all(|r| !r.complete));
}
#[test]
fn relation_projection_exact_partial_quotes_link_all_overlapping_originals() {
    let v = ArchiveVersion::V2;
    let docs = vec![
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":1,"type":"message","text":"before  цитата é 😀\r\n after"}),
        ),
        document(
            v,
            "private_channel",
            serde_json::json!({"id":2,"type":"message","text":"other  цитата é 😀\r\n original"}),
        ),
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":3,"type":"message","text":[{"type":"blockquote","text":" цитата é 😀\r\n"},"response"]}),
        ),
    ];
    let edges = resolve(v, &origin(), &docs).unwrap();
    assert!(edges[2].complete);
    let expected: std::collections::BTreeSet<_> =
        docs[..2].iter().map(|d| d.0.document_id.clone()).collect();
    assert_eq!(
        edges[2]
            .related
            .iter()
            .map(|r| r.document_id.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
}
#[test]
fn relation_projection_unresolved_or_mismatched_entities_are_not_reference_ready() {
    let v = ArchiveVersion::V2;
    let docs = vec![
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":1,"type":"message","text":"quotation","text_entities":[{"type":"blockquote","text":"quotation"}]}),
        ),
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":2,"type":"message","text":"actual","text_entities":[{"type":"plain","text":"different"}]}),
        ),
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":3,"type":"message","rich_message":{"text":"unsupported"}}),
        ),
    ];
    assert!(resolve(v, &origin(), &docs)
        .unwrap()
        .iter()
        .all(|r| !r.complete));
}

#[test]
fn relation_projection_self_reply_is_not_resolved_history() {
    let v = ArchiveVersion::V2;
    let docs = vec![document(
        v,
        "personal_chat",
        serde_json::json!({
            "id":1,"type":"message","text":"self","reply_to_message_id":1
        }),
    )];
    let edges = resolve(v, &origin(), &docs).unwrap();
    assert!(!edges[0].complete);
    assert!(edges[0].related.is_empty());
}
#[test]
fn relation_projection_legacy_bare_id_does_not_alias_a_different_peer_class() {
    let v = ArchiveVersion::V1;
    let docs = vec![
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":1,"type":"message","text":"user original"}),
        ),
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":2,"type":"message","text":"channel reply","reply_to_message_id":1,"reply_to_peer_id":"channel42"}),
        ),
    ];
    let edges = resolve(v, &origin(), &docs).unwrap();
    assert!(!edges[1].complete);
    assert!(edges[1].related.is_empty());
}
#[test]
fn relation_projection_rejects_oversized_quote_fanout_instead_of_dropping_edges() {
    let v = ArchiveVersion::V2;
    let mut docs: Vec<_> = (1..=130)
        .map(|id| {
            document(
                v,
                "personal_chat",
                serde_json::json!({
                    "id":id,"type":"message","text":format!("original {id} shared quotation")
                }),
            )
        })
        .collect();
    docs.push(document(
        v,
        "personal_chat",
        serde_json::json!({
            "id":131,"type":"message","text":[{"type":"blockquote","text":"shared quotation"}]
        }),
    ));
    assert!(resolve(v, &origin(), &docs).is_err());
}

#[test]
fn relation_projection_quote_edges_prevent_holdout_text_from_entering_chunks() {
    use crate::corpus_database::episodes::*;
    struct NoReference;
    impl EpisodeTokenizer for NoReference {
        fn version(&self) -> &str {
            "fixture:1"
        }
        fn count(&self, _: &str) -> Result<usize, String> {
            Err("holdout reached tokenizer".into())
        }
    }
    let v = ArchiveVersion::V2;
    let docs = vec![
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":1,"type":"message","text":"held original quoted words and tail"}),
        ),
        document(
            v,
            "private_channel",
            serde_json::json!({"id":2,"type":"message","text":"another quoted words source"}),
        ),
        document(
            v,
            "personal_chat",
            serde_json::json!({"id":3,"type":"message","text":[{"type":"blockquote","text":"quoted words"}," reply"]}),
        ),
    ];
    let edges = resolve(v, &origin(), &docs).unwrap();
    let input: Vec<_> = docs
        .iter()
        .zip(edges)
        .enumerate()
        .map(|(i, ((reference, doc), edge))| EpisodeDocument {
            reference: reference.clone(),
            source_key: doc.source_key.clone(),
            binding_id: "binding".into(),
            conversation_id: doc.resource_id.clone(),
            thread_id: None,
            timestamp_ms: Some(i as i64 * 1000),
            text: doc.text.clone(),
            related: edge.related,
            relations_complete: edge.complete,
            holdout: i == 0,
        })
        .collect();
    let split = EpisodePolicy {
        time_gap_ms: 1,
        holdout_basis_points: 0,
        seed: "fixture".into(),
    };
    let prepared = prepare("snapshot", &input, &split).unwrap();
    assert!(prepared
        .episodes()
        .iter()
        .all(|e| e.partition == Partition::Holdout));
    assert!(prepared
        .chunks(
            &NoReference,
            &ChunkPolicy {
                max_tokens: 100,
                max_bytes: 1024
            }
        )
        .unwrap()
        .is_empty());
}

#[test]
fn relation_projection_rejects_duplicate_rich_entity_type_that_hides_quote() {
    let v = ArchiveVersion::V2;
    let mut doc = document(
        v,
        "personal_chat",
        serde_json::json!({
            "id":1,"type":"message","text":[{"type":"blockquote","text":"quoted original"}]
        }),
    );
    doc.1.raw_json = doc.1.raw_json.replace(
        "\"type\":\"blockquote\"",
        "\"type\":\"blockquote\",\"type\":\"plain\"",
    );
    assert!(resolve(v, &origin(), &[doc]).is_err());
}
