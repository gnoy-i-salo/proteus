use super::*;
use crate::{
    corpus_database::RevisionReference,
    ingestion::{
        fingerprint::InputFingerprint, page_document::PageOrigin, preview::PreviewResource,
        preview_store::issue_page_preview,
    },
};
fn fixture() -> (StoredPreview, StoredRevision) {
    let document = page_document::normalize(
        PageOrigin::SavedPage {
            source_origin: format!("localArchivePath:1:sha256:{}", "a".repeat(64)),
            label: "  Точное имя 🦉.html".into(),
        },
        "text/html".into(),
        "<p>  Original\r\n&amp; текст</p>".into(),
        &mut || Ok(()),
    )
    .unwrap();
    let preview = issue_page_preview(
        document.origin.clone(),
        "savedPageHtml".into(),
        crate::ingestion::page_preview::ADAPTER_VERSION.into(),
        InputFingerprint {
            sha256: document.sha256.clone(),
            byte_length: document.original.len() as u64,
        },
        vec![PreviewResource {
            id: document.source_key.clone(),
            label: "Текст".into(),
            author_ids: vec![],
            estimated_documents: Some(1),
        }],
    )
    .unwrap();
    let revision = StoredRevision {
        reference: RevisionReference {
            document_id: document.id.clone(),
            revision_id: "fixture-revision".into(),
        },
        source_key: document.source_key.clone(),
        document_json: serde_json::to_string(&document).unwrap(),
        provenance: provenance(&preview, document.original.len() as u64),
    };
    (preview, revision)
}
#[test]
fn page_revision_requires_the_preview_original_identity_and_exact_provenance() {
    let (preview, original) = fixture();
    assert!(decode_revision(&preview, &original, &|| Ok(())).is_ok());
    for case in 0..10 {
        let mut revision = original.clone();
        match case {
            0 => revision.reference.document_id.push_str("-wrong"),
            1 => revision.source_key.push_str("-wrong"),
            2 => revision.provenance.acquisition = Acquisition::Archive,
            3 => revision.provenance.adapter_version = "savedPage:unknown".into(),
            4 => revision.provenance.source_format = "savedPageMarkdown".into(),
            5 => revision.provenance.original_byte_length = Some(0),
            6 => {
                revision.provenance.original_object_id = Some("fabricated-author-id".into());
                revision.provenance.limitations.remove(0);
            }
            7 => revision.provenance.limitations.clear(),
            _ => {
                let mut document =
                    page_document::decode(&revision.document_json, &mut || Ok(())).unwrap();
                if case == 8 {
                    if let PageOrigin::SavedPage { label, .. } = &mut document.origin {
                        *label = "Другой исходник.html".into();
                    }
                } else {
                    document.original = "<p>Changed original</p>".into();
                }
                document = page_document::normalize(
                    document.origin,
                    document.content_type,
                    document.original,
                    &mut || Ok(()),
                )
                .unwrap();
                revision.document_json = serde_json::to_string(&document).unwrap();
                revision.provenance = provenance(&preview, document.original.len() as u64);
            }
        }
        assert!(
            decode_revision(&preview, &revision, &|| Ok(())).is_err(),
            "case {case}"
        );
    }
}
#[test]
fn page_revision_checks_access_before_parse_and_during_extraction() {
    let (preview, mut revision) = fixture();
    let calls = std::cell::Cell::new(0);
    let result = decode_revision(&preview, &revision, &|| {
        calls.set(calls.get() + 1);
        if calls.get() >= 5 {
            Err("expired".into())
        } else {
            Ok(())
        }
    });
    assert_eq!(result.unwrap_err(), "expired");
    revision.document_json = "not JSON".into();
    assert_eq!(
        decode_revision(&preview, &revision, &|| Err("revoked".into())).unwrap_err(),
        "revoked"
    );
}
