use super::*;
use crate::{
    corpus_database::{
        episodes::SpanUnit, Acquisition, HashAlgorithm, JobPeriod, ProvenanceLimitation,
        RevisionReference, SourceProvenance,
    },
    ingestion::{
        telegram_archive_json::{TelegramChatHeader, TelegramMessageFrame},
        telegram_document::normalize_with_version,
        telegram_identity::ArchiveVersion,
    },
};
fn telegram(raw: &str) -> (CompletedAdapter, StoredRevision, JobBinding, ChunkSpan) {
    let version = ArchiveVersion::CURRENT;
    let origin = format!("localArchivePath:1:sha256:{}", "a".repeat(64));
    let doc = normalize_with_version(
        version,
        &origin,
        TelegramMessageFrame {
            chat: TelegramChatHeader {
                id: "42".into(),
                name: Some("  Чат é  ".into()),
                kind: "personal_chat".into(),
            },
            raw_json: raw.as_bytes().to_vec(),
            byte_start: 0,
            byte_end: raw.len() as u64,
        },
    )
    .unwrap();
    let binding = binding(JobProvider::Telegram, &doc.resource_id);
    let span = ChunkSpan {
        document_id: doc.id.clone(),
        revision_id: "verified-revision".into(),
        unit: SpanUnit::Utf8Bytes,
        start: 0,
        end: doc.text.len(),
    };
    let revision = StoredRevision {
        reference: RevisionReference {
            document_id: doc.id.clone(),
            revision_id: span.revision_id.clone(),
        },
        source_key: doc.source_key.clone(),
        document_json: serde_json::to_string(&doc).unwrap(),
        provenance: SourceProvenance {
            acquisition: Acquisition::Archive,
            adapter_version: version.adapter().into(),
            source_format: "telegramDesktopJson".into(),
            original_object_id: Some(doc.original_id),
            original_byte_length: Some(raw.len() as u64),
            hash_algorithm: HashAlgorithm::Sha256,
            limitations: vec![
                ProvenanceLimitation::AttachmentsUnavailable,
                ProvenanceLimitation::RelationsIncomplete,
            ],
        },
    };
    (
        CompletedAdapter::Telegram { version, origin },
        revision,
        binding,
        span,
    )
}
fn binding(provider: JobProvider, resource: &str) -> JobBinding {
    JobBinding {
        schema_version: 1,
        id: "binding".into(),
        persona_id: "person".into(),
        provider,
        account_id: None,
        resource_id: resource.into(),
        grant_id: "private-grant".into(),
        policy_version: "policy".into(),
        role: JobRole::Own,
        target_author_id: Some("user1".into()),
        allowed_context_author_ids: vec!["user2".into()],
        allow_unknown_context: true,
        period: JobPeriod {
            from: None,
            to: None,
            include_undated: true,
        },
    }
}
const PLAIN: &str = r#"{"id":7,"type":"message","from_id":"user1","from":"  Аня é ","date_unixtime":"1704067200","text":"  A\r\né 😀"}"#;
#[test]
fn exact_attributed_excerpt_preserves_labels_and_omits_private_scope() {
    let (adapter, revision, binding, mut span) = telegram(PLAIN);
    let before = revision.clone();
    span.start = 5;
    let excerpt = project(&adapter, &revision, &binding, &span, &|| Ok(())).unwrap();
    let json = serde_json::to_value(&excerpt).unwrap();
    assert_eq!(excerpt.text, "é 😀");
    assert_eq!(json["speakerRelation"], "target");
    assert_eq!(json["canSupportOwnWords"], true);
    assert_eq!(json["document"]["authorLabel"], "  Аня é ");
    assert_eq!(json["document"]["authorship"], "owner");
    assert_eq!(json["document"]["createdAt"], "2024-01-01T00:00:00Z");
    assert!(json["document"]["retrievedAt"].is_null());
    assert_eq!(json["sourceMetadata"]["resourceLabel"], "  Чат é  ");
    assert_eq!(
        json["sourceMetadata"]["authorLabels"],
        serde_json::json!(["  Аня é "])
    );
    assert_eq!(json["sourceMetadata"]["originalObjectId"], "7");
    assert_eq!(json["sourceMetadata"]["schemaVersion"], 1);
    assert!(!json.to_string().contains("private-grant"));
    assert!(json["document"].get("text").is_none());
    assert!(json.get("revisionId").is_none());
    assert_eq!(revision, before);
}
#[test]
fn context_and_unknown_authors_never_become_target_words() {
    for (author, relation, authorship) in [
        (Some("user2"), "context", "other"),
        (None, "unknown", "unknown"),
    ] {
        let mut raw: serde_json::Value = serde_json::from_str(PLAIN).unwrap();
        raw["from_id"] = serde_json::json!(author);
        let (adapter, revision, binding, span) = telegram(&raw.to_string());
        let json = serde_json::to_value(
            project(&adapter, &revision, &binding, &span, &|| Ok(())).unwrap(),
        )
        .unwrap();
        assert_eq!(json["speakerRelation"], relation);
        assert_eq!(json["document"]["authorship"], authorship);
        assert_eq!(json["canSupportOwnWords"], false);
    }
}
#[test]
fn known_quotes_and_forwards_block_whole_excerpt_own_words() {
    for (extra, kind) in [
        (
            r#", "forwarded_from":"  Автор  ","forwarded_from_id":"user9""#,
            "forward",
        ),
        (
            r#", "text_entities":[{"type":"blockquote","text":"  A\r\né 😀"}]"#,
            "quote",
        ),
    ] {
        let raw = format!("{}{extra}}}", &PLAIN[..PLAIN.len() - 1]);
        let (adapter, revision, binding, span) = telegram(&raw);
        let json = serde_json::to_value(
            project(&adapter, &revision, &binding, &span, &|| Ok(())).unwrap(),
        )
        .unwrap();
        assert_eq!(json["canSupportOwnWords"], false);
        assert!(json["relations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["kind"] == kind));
        if kind == "forward" {
            assert_eq!(json["relations"][0]["originalAuthor"]["label"], "  Автор  ");
            assert_eq!(json["relations"][0]["originalAuthor"]["id"], "user9");
        }
    }
}
#[test]
fn malformed_span_scope_or_original_cannot_be_projected() {
    let (adapter, original, original_binding, original_span) = telegram(PLAIN);
    for case in 0..8 {
        let mut revision = original.clone();
        let mut binding = original_binding.clone();
        let mut span = original_span.clone();
        match case {
            0 => span.start = 7,
            1 => span.end += 1,
            2 => span.end = 0,
            3 => span.revision_id = "other".into(),
            4 => span.document_id = "other".into(),
            5 => binding.resource_id = "other".into(),
            6 => binding.provider = JobProvider::Web,
            _ => {
                let mut value: serde_json::Value =
                    serde_json::from_str(&revision.document_json).unwrap();
                value["authorLabel"] = serde_json::json!("Another");
                revision.document_json = value.to_string();
            }
        }
        assert!(
            project(&adapter, &revision, &binding, &span, &|| Ok(())).is_err(),
            "case {case}"
        );
    }
    assert!(project(
        &adapter,
        &original,
        &original_binding,
        &original_span,
        &|| Err("revoked".into())
    )
    .is_err());
}
fn page(url: bool) -> (CompletedAdapter, StoredRevision, JobBinding, ChunkSpan) {
    use crate::ingestion::{
        fingerprint::InputFingerprint,
        page_document::{normalize, PageOrigin},
        preview::PreviewResource,
        preview_store::issue_page_preview,
    };
    let origin = if url {
        PageOrigin::SelectedUrl {
            selected_url: "https://example.org/a".into(),
            final_url: "https://example.org/a".into(),
            redirects: vec![],
            retrieved_at: "2026-11-07T12:34:56Z".into(),
        }
    } else {
        PageOrigin::SavedPage {
            source_origin: format!("localArchivePath:1:sha256:{}", "b".repeat(64)),
            label: "  Статья.html".into(),
        }
    };
    let doc = normalize(origin.clone(), "text/html".into(), "<html><head><title>Точный заголовок</title><meta name=\"author\" content=\"Аня\"><meta name=\"author\" content=\"Борис\"></head><body><p>é 😀 Original</p></body></html>".into(), &mut || Ok(())).unwrap();
    let preview = issue_page_preview(
        origin,
        if url {
            "selectedUrlHtml"
        } else {
            "savedPageHtml"
        }
        .into(),
        if url {
            crate::ingestion::page_preview::URL_ADAPTER_VERSION
        } else {
            crate::ingestion::page_preview::ADAPTER_VERSION
        }
        .into(),
        InputFingerprint {
            sha256: doc.sha256.clone(),
            byte_length: doc.original.len() as u64,
        },
        vec![PreviewResource {
            id: doc.source_key.clone(),
            label: "Статья".into(),
            author_ids: vec![],
            estimated_documents: Some(1),
        }],
    )
    .unwrap();
    let binding = binding(
        if url {
            JobProvider::Web
        } else {
            JobProvider::SavedPage
        },
        &doc.source_key,
    );
    let span = ChunkSpan {
        document_id: doc.id.clone(),
        revision_id: "page-revision".into(),
        unit: SpanUnit::Utf8Bytes,
        start: 0,
        end: doc.text.len(),
    };
    let revision = StoredRevision {
        reference: RevisionReference {
            document_id: doc.id.clone(),
            revision_id: span.revision_id.clone(),
        },
        source_key: doc.source_key.clone(),
        document_json: serde_json::to_string(&doc).unwrap(),
        provenance: crate::ingestion::page_source::provenance(&preview, doc.original.len() as u64),
    };
    (
        CompletedAdapter::Page(Box::new(preview)),
        revision,
        binding,
        span,
    )
}
#[test]
fn pages_preserve_plural_labels_without_invented_identity_or_read_time() {
    for url in [false, true] {
        let (adapter, revision, binding, span) = page(url);
        let before = revision.clone();
        let json = serde_json::to_value(
            project(&adapter, &revision, &binding, &span, &|| Ok(())).unwrap(),
        )
        .unwrap();
        assert_eq!(
            json["sourceMetadata"]["authorLabels"],
            serde_json::json!(["Аня", "Борис"])
        );
        assert_eq!(json["sourceMetadata"]["title"], "Точный заголовок");
        assert!(json["document"]["authorId"].is_null());
        assert!(json["document"]["authorLabel"].is_null());
        assert_eq!(json["speakerRelation"], "unknown");
        assert_eq!(json["canSupportOwnWords"], false);
        assert_eq!(
            json["document"]["retrievedAt"],
            if url {
                serde_json::json!("2026-11-07T12:34:56Z")
            } else {
                serde_json::Value::Null
            }
        );
        assert_eq!(revision, before);
    }
}
