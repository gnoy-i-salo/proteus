use super::*;
use crate::memory::types::{MemoryItem, MemorySnapshot};
fn publish_two(
    bound: &mut MemorySession,
) -> crate::memory::publication::ResearchPublicationReceipt {
    let mut value = draft(bound);
    let mut second = value.items[0].clone();
    second.draft_id = "draft:second".into();
    second.text = "  заметка é 😀\r\n".into();
    value.items.push(second);
    bound.publish_research(&value, || 70004).unwrap()
}
#[test]
fn published_memory_reloads_exact_items_and_charges_unique_native_episodes() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let publication = publish_two(&mut bound);
    drop(bound);
    let mut reader = session(&sources, &snapshot, limits()).unwrap();
    let ids: Vec<_> = publication
        .snapshot
        .item_ids
        .iter()
        .rev()
        .cloned()
        .collect();
    let read = reader
        .read_published_memory(&publication.snapshot.id, &ids, || 70005)
        .unwrap();
    assert_eq!(read.memory_snapshot_id(), publication.snapshot.id);
    assert_eq!(
        read.items()
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(read.items()[0].text, "  заметка é 😀\r\n");
    assert_eq!(read.items()[1].text, "  контекст\r\n");
    assert_eq!(reader.usage().0, 1);
    assert_eq!(read.receipts().len(), 1);
    assert!(read.receipts()[0]
        .sources
        .iter()
        .all(|s| s.document.partition == "reference"));
    assert!(reader.usage().1.source_bytes > 0);
    for bad in [
        vec![ids[0].clone(), ids[0].clone()],
        vec!["foreign".into()],
        vec![" ".into()],
        vec!["x".repeat(257)],
    ] {
        assert!(reader
            .read_published_memory(&publication.snapshot.id, &bad, || 70005)
            .is_err());
        assert_eq!(reader.usage().0, 1);
    }
    drop(reader);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn published_memory_rechecks_grants_index_cancellation_and_budget() {
    for kind in ["revoke", "index", "cancel", "budget"] {
        let (sources, dir) = local();
        let (op, snapshot, _) = indexed(&sources, &dir, 0);
        let grant = enable_model(&sources, &op);
        let mut bound = session(&sources, &snapshot, limits()).unwrap();
        let publication = publish_two(&mut bound);
        drop(bound);
        let mut budget = limits();
        budget.max_calls = if kind == "budget" { 1 } else { 3 };
        let mut reader = session(&sources, &snapshot, budget).unwrap();
        reader
            .read_published_memory(
                &publication.snapshot.id,
                &publication.snapshot.item_ids,
                || 70005,
            )
            .unwrap();
        match kind {
            "revoke" => {
                sources
                    .with_ingestion_database(&op, |db| db.remove(&grant))
                    .unwrap();
            }
            "index" => {
                rebuild(
                    &sources,
                    &op,
                    &IndexInput {
                        job_id: "job",
                        snapshot_id: &snapshot.id,
                        max_duration_ms: 60000,
                    },
                    || 70006,
                )
                .unwrap();
            }
            "cancel" => reader.cancel(),
            _ => {}
        }
        assert!(
            reader
                .read_published_memory(
                    &publication.snapshot.id,
                    &publication.snapshot.item_ids,
                    || 70006
                )
                .is_err(),
            "{kind}"
        );
        drop(reader);
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

// Seed structurally valid storage directly to demonstrate that storage membership
// cannot turn holdout, a forged source assertion or a partial UTF8 span into access.
fn seed_untrusted_memory(
    sources: &Sources,
    op: &crate::ingestion::operations::Operation,
    snapshot: &str,
    kind: &str,
) -> MemorySnapshot {
    sources
        .with_ingestion_database(op, |db| {
            let prepared = db.prepared_snapshot(snapshot)?;
            let mut selected = None;
            for episode_id in &prepared.episode_ids {
                let episode = db.prepared_episode(snapshot, episode_id)?;
                for reference in &episode.document_refs {
                    let revision = db.revision(reference)?;
                    let value: serde_json::Value =
                        serde_json::from_str(&revision.document_json).unwrap();
                    let text = value["text"].as_str().unwrap();
                    if text.contains("контекст") {
                        selected = Some((
                            episode.id.clone(),
                            text.to_owned(),
                            crate::corpus_database::episodes::ChunkSpan {
                                document_id: reference.document_id.clone(),
                                revision_id: reference.revision_id.clone(),
                                unit: crate::corpus_database::episodes::SpanUnit::Utf8Bytes,
                                start: 0,
                                end: text.len(),
                            },
                        ));
                    }
                }
            }
            let (episode_id, text, span) =
                selected.expect("fixture includes the context original even in holdout");
            let policy = db.job("job")?.unwrap().state.selection.bindings[0]
                .policy_version
                .clone();
            let corpus = if kind == "corpus" {
                db.prepared_source_snapshot_id(snapshot)?
            } else {
                snapshot.to_owned()
            };
            let mut item = MemoryItem {
                schema_version: 1,
                id: "untrusted:item".into(),
                snapshot_id: corpus.clone(),
                kind: MemoryItemKind::ReportedAssertion,
                text,
                source_spans: vec![span],
                episode_ids: vec![episode_id],
                event_at: None,
                observed_at: "1970-01-01T00:01:10.004Z".into(),
                support: MemoryItemSupport::Reported,
                contradiction_ids: vec![],
                limitations: vec![],
                extraction_version: "test:1".into(),
                policy_version: policy,
            };
            match kind {
                "utf8" => item.source_spans[0].start += 3,
                "episode" => item.episode_ids[0] = "foreign".into(),
                "policy" => item.policy_version = "foreign".into(),
                "own" => {
                    item.kind = MemoryItemKind::Procedure;
                    item.support = MemoryItemSupport::Observed;
                }
                _ => {}
            }
            let memory = MemorySnapshot {
                schema_version: 1,
                id: "untrusted:snapshot".into(),
                corpus_snapshot_id: corpus,
                item_ids: vec![item.id.clone()],
                extraction_version: item.extraction_version.clone(),
                created_at: item.observed_at.clone(),
            };
            db.publish_memory_snapshot_checked(
                if kind == "owner" {
                    "foreign"
                } else {
                    "personA"
                },
                &memory,
                &[item],
                &|| Ok(()),
            )?;
            Ok(memory)
        })
        .unwrap()
}
#[test]
fn published_memory_never_promotes_holdout_or_unverified_support_to_source_access() {
    for (holdout, kind) in [
        (0, "valid"),
        (10000, "holdout"),
        (0, "utf8"),
        (0, "episode"),
        (0, "policy"),
        (0, "own"),
        (0, "owner"),
        (0, "corpus"),
    ] {
        let (sources, dir) = local();
        let (op, snapshot, _) = indexed(&sources, &dir, holdout);
        enable_model(&sources, &op);
        let memory = seed_untrusted_memory(&sources, &op, &snapshot.id, kind);
        let mut reader = session(&sources, &snapshot, limits()).unwrap();
        assert_eq!(
            reader
                .read_published_memory(&memory.id, &memory.item_ids, || 70005)
                .is_ok(),
            kind == "valid",
            "{kind}"
        );
        drop(reader);
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[path = "memory-fabric-session-tests.rs"]
mod fabric_session_tests;

#[test]
fn published_memory_late_cancellation_and_model_expiry_discard_the_result() {
    use std::cell::Cell;
    for mode in ["cancel", "expiry"] {
        let (sources, dir) = local();
        let (op, snapshot, _) = indexed(&sources, &dir, 0);
        enable_model(&sources, &op);
        let mut researcher = session(&sources, &snapshot, limits()).unwrap();
        let publication = publish_two(&mut researcher);
        drop(researcher);
        let calls = Cell::new(0);
        let mut probe = session(&sources, &snapshot, limits()).unwrap();
        configure(&mut probe);
        probe
            .read_published_memory(
                &publication.snapshot.id,
                &publication.snapshot.item_ids,
                || {
                    calls.set(calls.get() + 1);
                    70005
                },
            )
            .unwrap();
        let last = calls.get();
        assert!(last > 2);
        drop(probe);
        let mut reader = session(&sources, &snapshot, limits()).unwrap();
        configure(&mut reader);
        calls.set(0);
        let result = reader.read_published_memory(
            &publication.snapshot.id,
            &publication.snapshot.item_ids,
            || {
                calls.set(calls.get() + 1);
                if calls.get() == last {
                    if mode == "cancel" {
                        sources.cancel_ingestion_operation("memory").unwrap();
                    } else {
                        return 71003;
                    }
                }
                70005
            },
        );
        assert!(result.is_err(), "{mode}");
        assert_eq!(
            reader.usage().0,
            1,
            "failure happens after source retrieval"
        );
        drop(reader);
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
