use super::*;
use serde_json::{json, Value};

pub(super) fn fixture() -> ResearchPublication {
    serde_json::from_value(json!({
        "schemaVersion": 1, "inputSha256": "a".repeat(64),
        "receipt": {
            "schemaVersion": 1, "id": "publication:1",
            "snapshot": {"schemaVersion": 1,"id": "memory:1", "corpusSnapshotId": "corpus:1",
                "itemIds": ["item:1"], "extractionVersion": "extract:1", "createdAt": "2026-11-09T00:00:00Z"},
            "itemIds": [{"draftId": "draft:1", "itemId": "item:1"}],
            "coverage": {"loadedDocuments": 2, "indexedDocuments": 2, "analyzedDocuments": 1,
                "usedDocuments": 1, "totalDocuments": null, "loadedBytes": 64,
                "complete": false, "limitations": ["  Один источник\r\n😀"]},
            "analyzedSpans": [{"documentId": "doc:1", "revisionId": "rev:1", "unit": "utf8Bytes", "start": 0, "end": 4}],
            "publishedAt": "2026-11-09T00:00:00Z"
        },
        "retrievals": [{"schemaVersion": 2, "operation": "recall", "id": "retrieval:1", "snapshotId": "corpus:1",
            "policyVersion": "policy:1", "retrievalVersion": "nativeMemoryRetrieval:1", "indexGeneration": 1,
            "embeddingFingerprint": null, "mode": "lexical", "hits": [{"chunkId": "chunk:1", "episodeId": "episode:1",
                "spans": [{"documentId": "doc:1", "revisionId": "rev:1", "unit": "utf8Bytes", "start": 0, "end": 4}],
                "lexicalRank": 1, "semanticRank": null, "fusionScore": 0.01}],
            "elapsedMs": 3, "sourceBytes": 4, "contextTokens": 1}],
        "modelContexts": [{"schemaVersion": 1, "leaseId": "modelContext:1", "invocationId": "invocation-1",
            "provider": "kimi", "model": "kimi-k2", "effort": "high", "operation": "corpus-analysis",
            "inputSha256": "a".repeat(64), "inputBytes": 100, "contextSha256": "b".repeat(64),
            "receiptIds": ["retrieval:1"], "formatAttempt": 1, "completedAtMs": 1000}]
    })).unwrap()
}

#[test]
fn publication_roundtrip_preserves_exact_metadata_and_required_nullable_fields() {
    let original = fixture();
    original.validate().unwrap();
    let wire = serde_json::to_string(&original).unwrap();
    assert!(!wire.contains("schema_version"));
    assert_eq!(
        serde_json::from_str::<ResearchPublication>(&wire).unwrap(),
        original
    );
    for pointer in [
        "/receipt/coverage",
        "/retrievals/0",
        "/retrievals/0/hits/0",
        "/modelContexts/0",
    ] {
        let mut value = serde_json::to_value(&original).unwrap();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), json!(true));
        assert!(
            serde_json::from_value::<ResearchPublication>(value).is_err(),
            "{pointer}"
        );
    }
    for (pointer, field) in [
        ("/receipt/coverage", "totalDocuments"),
        ("/retrievals/0", "embeddingFingerprint"),
        ("/retrievals/0/hits/0", "semanticRank"),
    ] {
        let mut value = serde_json::to_value(&original).unwrap();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(
            serde_json::from_value::<ResearchPublication>(value).is_err(),
            "{field}"
        );
    }
}

#[test]
fn rejects_forged_versions_counts_mapping_and_audit_links() {
    let cases: &[(&str, Value)] = &[
        ("/schemaVersion", json!(2)),
        ("/inputSha256", json!("bad")),
        ("/receipt/schemaVersion", json!(2)),
        ("/receipt/id", json!("")),
        ("/receipt/publishedAt", json!("bad")),
        ("/receipt/itemIds/0/itemId", json!("alien")),
        ("/receipt/itemIds/0/draftId", json!("")),
        ("/receipt/coverage/analyzedDocuments", json!(2)),
        ("/receipt/coverage/usedDocuments", json!(2)),
        ("/receipt/coverage/indexedDocuments", json!(3)),
        ("/receipt/coverage/totalDocuments", json!(1)),
        (
            "/receipt/coverage/loadedBytes",
            json!(9_007_199_254_740_992u64),
        ),
        ("/receipt/analyzedSpans/0/end", json!(3)),
        ("/retrievals/0/schemaVersion", json!(1)),
        ("/retrievals/0/snapshotId", json!("foreign")),
        ("/retrievals/0/retrievalVersion", json!("unknown")),
        ("/retrievals/0/mode", json!("hybrid")),
        ("/retrievals/0/elapsedMs", json!(-1)),
        ("/retrievals/0/sourceBytes", json!(3)),
        ("/retrievals/0/hits/0/lexicalRank", json!(0)),
        ("/modelContexts/0/schemaVersion", json!(2)),
        ("/modelContexts/0/receiptIds/0", json!("missing")),
        ("/modelContexts/0/provider", json!("unknown")),
        ("/modelContexts/0/operation", json!("persona-reply")),
        ("/modelContexts/0/inputSha256", json!("bad")),
        ("/modelContexts/0/formatAttempt", json!(3)),
        ("/modelContexts/0/inputBytes", json!(0)),
        ("/modelContexts/0/completedAtMs", json!(-1)),
    ];
    for (pointer, replacement) in cases {
        let mut value = serde_json::to_value(fixture()).unwrap();
        *value.pointer_mut(pointer).unwrap() = replacement.clone();
        let decoded: ResearchPublication = serde_json::from_value(value).unwrap();
        assert!(
            decoded.validate().is_err(),
            "accepted {pointer}: {replacement}"
        );
    }
}

#[test]
fn duplicate_receipts_contexts_spans_and_draft_ids_are_rejected() {
    let mut value = fixture();
    value.retrievals.push(value.retrievals[0].clone());
    assert!(value.validate().is_err());
    let mut value = fixture();
    value.model_contexts.push(value.model_contexts[0].clone());
    assert!(value.validate().is_err());
    let mut value = fixture();
    value
        .receipt
        .analyzed_spans
        .push(value.receipt.analyzed_spans[0].clone());
    assert!(value.validate().is_err());
    let mut value = fixture();
    value.model_contexts.clear();
    assert!(
        value.validate().is_err(),
        "unconsumed receipt must not claim analysis"
    );
}

#[test]
fn empty_analysis_is_valid_and_deduplicated_spans_count_documents_not_hits() {
    let mut value = fixture();
    let mut second = value.retrievals[0].clone();
    second.id = "retrieval:2".into();
    value.retrievals.push(second);
    value.model_contexts[0]
        .receipt_ids
        .push("retrieval:2".into());
    value.validate().unwrap();
    value.receipt.snapshot.item_ids.clear();
    value.receipt.item_ids.clear();
    value.receipt.analyzed_spans.clear();
    value.retrievals.clear();
    value.model_contexts.clear();
    value.receipt.coverage.analyzed_documents = 0;
    value.receipt.coverage.used_documents = 0;
    value.validate().unwrap();
}

#[test]
fn publication_matches_shared_coverage_completion_contract() {
    let mut value = fixture();
    value.receipt.coverage.complete = true;
    value.receipt.coverage.total_documents = Some(3);
    assert!(
        value.validate().is_err(),
        "complete acquisition cannot omit a known document"
    );
    value.receipt.coverage.total_documents = Some(2);
    value.validate().unwrap();
    assert_eq!(
        value.receipt.coverage.analyzed_documents, 1,
        "acquisition completion is not complete analysis"
    );
}

#[test]
fn publication_matches_shared_recall_rank_contract() {
    let mut value = fixture();
    value.retrievals[0].hits[0].lexical_rank = None;
    assert!(
        value.validate().is_err(),
        "ranked recall must have a lexical or semantic rank"
    );
    value.retrievals[0].operation = "readEpisode".into();
    value.retrievals[0].hits[0].fusion_score = 0.0;
    value.validate().unwrap();
}
