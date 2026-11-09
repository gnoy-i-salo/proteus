use super::*;
use serde_json::{json, Value};
fn fixture() -> Value {
    let text = "  Я\r\n";
    let first = json!({
        "draftId":"draft:α", "kind":"episode", "text":text,
        "sourceSpans":[{"documentId":"doc:1","revisionId":"rev:1","unit":"utf8Bytes","start":0,"end":text.len()}],
        "episodeIds":["episode:1"],"eventAt":null,"support":"reported",
        "contradictions":[{"kind":"published","itemId":"memory:old"}],"limitations":["  Один источник\r\n"]
    });
    let mut second = first.clone();
    second["draftId"] = json!("draft:b");
    second["contradictions"] = json!([{"kind":"draft","draftId":"draft:α"}]);
    json!({"schemaVersion":1,"extractionVersion":"extract-v1","items":[first,second],
        "analyzedRefs":[{"documentId":"doc:1","revisionId":"rev:1"}],"contextLeaseIds":["modelContext:abc"]})
}
fn identity() -> PublicationIdentity {
    PublicationIdentity {
        memory_snapshot_id: "memorySnapshot:native".into(),
        corpus_snapshot_id: "corpus:native".into(),
        item_ids: vec!["memory:native1".into(), "memory:native2".into()],
        policy_version: "native-policy-v1".into(),
        published_at: "2026-11-09T00:00:00Z".into(),
    }
}
#[test]
fn strict_draft_roundtrip_preserves_exact_text_and_accepts_empty_analysis() {
    let value = fixture();
    let draft = decode(&value.to_string()).unwrap();
    assert_eq!(serde_json::to_value(&draft).unwrap(), value);
    let empty = json!({"schemaVersion":1,"extractionVersion":"v1","items":[],"analyzedRefs":[],"contextLeaseIds":[]});
    assert_eq!(
        serde_json::to_value(decode(&empty.to_string()).unwrap()).unwrap(),
        empty
    );
    assert!(decode(&" ".repeat(MAX_DRAFT_BYTES + 1)).is_err());
}
#[test]
fn draft_rejects_unknown_versions_fields_aliases_and_missing_nullable_values() {
    for (pointer, value) in [
        ("/schemaVersion", json!(2)),
        ("/extractionVersion", json!("")),
        ("/items/0/id", json!("forged")),
        ("/items/0/schemaVersion", json!(1)),
        ("/items/0/snapshotId", json!("forged")),
        ("/items/0/observedAt", json!("forged")),
        ("/items/0/policyVersion", json!("forged")),
        ("/items/0/kind", json!("assertion")),
        ("/items/0/contradictions/0/draftId", json!("mixed-alias")),
    ] {
        let mut bad = fixture();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        bad.pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(key.into(), value);
        assert!(decode(&bad.to_string()).is_err(), "accepted {pointer}");
    }
    let mut missing = fixture();
    missing["items"][0]
        .as_object_mut()
        .unwrap()
        .remove("eventAt");
    assert!(decode(&missing.to_string()).is_err());
}
#[test]
fn draft_rejects_invalid_spans_ids_dates_bounds_and_contradiction_graphs() {
    for (pointer, value) in [
        ("/items/0/text", json!("")),
        ("/items/0/text", json!("я".repeat(32769))),
        ("/items/0/sourceSpans/0/end", json!(0)),
        ("/items/0/sourceSpans/0/end", json!(9007199254740992u64)),
        ("/items/0/episodeIds", json!([])),
        ("/items/0/eventAt", json!("2026-02-30T00:00:00Z")),
        ("/items/0/limitations", json!([" "])),
        ("/items/0/draftId", json!("draft:b")),
        (
            "/items/1/contradictions",
            json!([{"kind":"draft","draftId":"draft:b"}]),
        ),
        (
            "/items/1/contradictions",
            json!([{"kind":"draft","draftId":"missing"}]),
        ),
        (
            "/items/1/contradictions",
            json!([{"kind":"published","itemId":"x"},{"kind":"published","itemId":"x"}]),
        ),
        ("/contextLeaseIds", json!(["same", "same"])),
        ("/contextLeaseIds", json!(["кириллица"])),
        ("/contextLeaseIds", json!(["x".repeat(129)])),
        ("/contextLeaseIds", json!(vec!["x"; 1001])),
        (
            "/analyzedRefs",
            json!([{"documentId":"doc:1","revisionId":"rev:1"},{"documentId":"doc:1","revisionId":"rev:1"}]),
        ),
    ] {
        let mut bad = fixture();
        *bad.pointer_mut(pointer).unwrap() = value;
        assert!(decode(&bad.to_string()).is_err(), "accepted {pointer}");
    }
    let mut too_many = fixture();
    too_many["items"] = json!(vec![fixture()["items"][0].clone(); 1001]);
    assert!(decode(&too_many.to_string()).is_err());
}
#[test]
fn materialization_uses_native_identity_and_resolves_batch_references_without_rewriting_text() {
    let draft = decode(&fixture().to_string()).unwrap();
    let native = identity();
    let bundle = draft.materialize(&native).unwrap();
    assert_eq!(bundle.snapshot.id, native.memory_snapshot_id);
    assert_eq!(
        bundle.snapshot.corpus_snapshot_id,
        native.corpus_snapshot_id
    );
    assert_eq!(bundle.snapshot.item_ids, native.item_ids);
    assert_eq!(bundle.items[0].contradiction_ids, vec!["memory:old"]);
    assert_eq!(bundle.items[1].contradiction_ids, vec!["memory:native1"]);
    for (item, original) in bundle.items.iter().zip(&draft.items) {
        assert_eq!(item.schema_version, 1);
        assert_eq!(item.snapshot_id, native.corpus_snapshot_id);
        assert_eq!(item.policy_version, native.policy_version);
        assert_eq!(item.observed_at, native.published_at);
        assert_eq!(item.text, original.text);
        assert_eq!(item.source_spans, original.source_spans);
        assert_eq!(item.limitations, original.limitations);
        assert_eq!(item.extraction_version, draft.extraction_version);
    }
    assert_eq!(draft.materialize(&native).unwrap(), bundle);
}
#[test]
fn materialization_rejects_invalid_native_identity_and_published_refs_colliding_with_new_ids() {
    let draft = decode(&fixture().to_string()).unwrap();
    for mode in 0..5 {
        let mut native = identity();
        match mode {
            0 => native.item_ids.pop().map(|_| ()).unwrap(),
            1 => native.item_ids[1] = native.item_ids[0].clone(),
            2 => native.policy_version.clear(),
            3 => native.published_at = "invalid".into(),
            _ => native.item_ids[0] = "memory:old".into(),
        }
        assert!(draft.materialize(&native).is_err());
    }
}

#[test]
fn typed_drafts_obey_total_encoded_byte_and_item_limits() {
    let mut draft = decode(&fixture().to_string()).unwrap();
    let prototype = draft.items[0].clone();
    draft.items = (0..1000)
        .map(|i| {
            let mut item = prototype.clone();
            item.draft_id = format!("draft:{i}");
            item
        })
        .collect();
    assert!(draft.validate().is_ok());
    let mut extra = prototype.clone();
    extra.draft_id = "draft:extra".into();
    draft.items.push(extra);
    assert!(draft.validate().is_err());
    draft.items = (0..300)
        .map(|i| {
            let mut item = prototype.clone();
            item.draft_id = format!("draft:{i}");
            item.text = "x".repeat(65536);
            item
        })
        .collect();
    assert!(
        draft.validate().is_err(),
        "typed path bypassed cumulative bytes"
    );
}
