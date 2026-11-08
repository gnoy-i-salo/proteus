use super::*;
use crate::memory::session::{RecordedRetrieval, RetrievalReceipt};

fn record(id: &str, metadata: &str) -> RecordedRetrieval {
    RecordedRetrieval {
        receipt: RetrievalReceipt {
            schema_version: 2,
            operation: "recall",
            id: id.into(),
            snapshot_id: metadata.into(),
            policy_version: "policy:1".into(),
            retrieval_version: "nativeMemoryRetrieval:1",
            index_generation: 1,
            embedding_fingerprint: None,
            mode: "lexical",
            hits: vec![],
            elapsed_ms: 0,
            source_bytes: 0,
            context_tokens: 0,
        },
        sources: vec![],
    }
}

#[test]
fn retains_exact_receipts_in_requested_order_without_accepting_foreign_or_duplicate_ids() {
    let mut history = ReceiptHistory::default();
    let first = history.insert(record("native-a", "  Русский\r\n")).unwrap();
    history.insert(record("native-b", "b")).unwrap();
    let selected = history
        .select(&["native-b".into(), "native-a".into()])
        .unwrap();
    assert_eq!(selected[0].receipt.id, "native-b");
    assert_eq!(selected[1].receipt.snapshot_id, first.receipt.snapshot_id);
    assert!(history
        .select(&["native-a".into(), "foreign".into()])
        .is_err());
    assert!(history
        .select(&["native-a".into(), "native-a".into()])
        .is_err());
    assert!(history.insert(record("native-a", "replacement")).is_err());
    assert!(history.select(&[]).unwrap().is_empty());
    assert_eq!(
        history.select(&["native-a".into()]).unwrap()[0]
            .receipt
            .snapshot_id,
        "  Русский\r\n"
    );
}

#[test]
fn count_limit_rejects_new_receipts_without_evicting_existing_ones() {
    let mut history = ReceiptHistory::default();
    for n in 0..1000 {
        history.insert(record(&format!("id-{n}"), "small")).unwrap();
    }
    assert!(history.insert(record("overflow", "small")).is_err());
    assert!(history.select(&vec!["id-0".into(); 1001]).is_err());
    assert_eq!(
        history.select(&["id-0".into()]).unwrap()[0]
            .receipt
            .snapshot_id,
        "small"
    );
}

#[test]
fn cumulative_serialized_bytes_include_metadata_and_json_escaping() {
    let mut history = ReceiptHistory::default();
    // Control characters expand sixfold in JSON. Counting only sourceBytes or
    // string length would admit more retained bytes than the 16 MiB limit.
    let metadata = "\u{0001}".repeat(1024 * 1024);
    history.insert(record("one", &metadata)).unwrap();
    history.insert(record("two", &metadata)).unwrap();
    assert!(history.insert(record("three", &metadata)).is_err());
    assert!(history.select(&["three".into()]).is_err());
    history
        .insert(record("small-after-rejection", "exact\r\n"))
        .unwrap();
    assert_eq!(
        history.select(&["one".into()]).unwrap()[0]
            .receipt
            .snapshot_id,
        metadata
    );
}
