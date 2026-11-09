use super::*;
use crate::memory::publication::{
    PublicationCoverage, PublishedItemId, ResearchPublication, ResearchPublicationReceipt,
};
use crate::memory::session::{CompletedModelContext, ReceiptHit, RetrievalReceipt};

fn audit(snapshot: &MemorySnapshot, items: &[MemoryItem]) -> ResearchPublication {
    let spans = items
        .iter()
        .flat_map(|i| i.source_spans.clone())
        .collect::<Vec<_>>();
    ResearchPublication {
        schema_version: 1,
        input_sha256: "a".repeat(64),
        receipt: ResearchPublicationReceipt {
            schema_version: 1,
            id: "publication:1".into(),
            snapshot: snapshot.clone(),
            item_ids: items
                .iter()
                .enumerate()
                .map(|(i, item)| PublishedItemId {
                    draft_id: format!("draft:{i}"),
                    item_id: item.id.clone(),
                })
                .collect(),
            coverage: PublicationCoverage {
                loaded_documents: 1,
                indexed_documents: 1,
                analyzed_documents: 1,
                used_documents: 1,
                total_documents: None,
                loaded_bytes: 1024,
                complete: false,
                limitations: vec!["Неполная история\r\n😀".into()],
            },
            analyzed_spans: spans.clone(),
            published_at: snapshot.created_at.clone(),
        },
        retrievals: vec![RetrievalReceipt {
            schema_version: 2,
            operation: "recall".into(),
            id: "retrieval:1".into(),
            snapshot_id: snapshot.corpus_snapshot_id.clone(),
            policy_version: "policy-v1".into(),
            retrieval_version: "nativeMemoryRetrieval:1".into(),
            index_generation: 1,
            embedding_fingerprint: None,
            mode: "lexical".into(),
            hits: items
                .iter()
                .enumerate()
                .map(|(i, item)| ReceiptHit {
                    chunk_id: format!("chunk:{i}"),
                    episode_id: item.episode_ids[0].clone(),
                    spans: item.source_spans.clone(),
                    lexical_rank: Some(i + 1),
                    semantic_rank: None,
                    fusion_score: 0.1,
                })
                .collect(),
            elapsed_ms: 1,
            source_bytes: spans.iter().map(|s| s.end - s.start).sum(),
            context_tokens: 1,
        }],
        model_contexts: vec![CompletedModelContext {
            schema_version: 1,
            lease_id: "modelContext:1".into(),
            invocation_id: "invocation-1".into(),
            provider: "codex".into(),
            model: "gpt-6".into(),
            effort: "high".into(),
            operation: "corpus-analysis".into(),
            input_sha256: "b".repeat(64),
            input_bytes: 1000,
            context_sha256: "c".repeat(64),
            receipt_ids: vec!["retrieval:1".into()],
            format_attempt: 1,
            completed_at_ms: 1000,
        }],
    }
}
fn publish_audit(
    db: &mut CorpusDatabase,
    audit: &ResearchPublication,
    items: &[MemoryItem],
) -> Result<StoredResearchPublication, String> {
    db.publish_research_checked(PERSONA, audit, items, &"a".repeat(64), &|| Ok(()))
}
fn read_audit(db: &CorpusDatabase) -> Result<StoredResearchPublication, String> {
    db.read_research_publication_checked("publication:1", PERSONA, &|| Ok(()))
}

#[test]
fn publication_reopens_encrypted_and_replays_exactly_without_rewriting_sources() {
    let (mut db, dir) = setup();
    let (_, reference, snapshot, items) = single_bundle(&mut db, "corpus-a");
    let original = db.revision(&reference).unwrap();
    let audit = audit(&snapshot, &items);
    let expected = publish_audit(&mut db, &audit, &items).unwrap();
    assert_eq!(expected.publication, audit);
    assert_eq!(expected.bundle.items, items);
    assert_eq!(publish_audit(&mut db, &audit, &items).unwrap(), expected);
    drop(db);
    let bytes = std::fs::read(dir.join("corpus.db")).unwrap();
    assert!(!bytes.starts_with(b"SQLite format 3"));
    assert!(!bytes
        .windows(items[0].text.len())
        .any(|v| v == items[0].text.as_bytes()));
    let mut db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(read_audit(&db).unwrap(), expected);
    assert_eq!(publish_audit(&mut db, &audit, &items).unwrap(), expected);
    assert_eq!(db.revision(&reference).unwrap(), original);
    assert!(db
        .read_research_publication_checked("publication:1", OTHER_PERSONA, &|| Ok(()))
        .is_err());
    db.remove("record-corpus-a").unwrap();
    assert!(read_audit(&db).is_err());
    assert_eq!(memory_row_count(&db.connection, "memory_publications"), 0);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn conflicting_publication_input_or_audit_cannot_replace_or_leave_a_bundle() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    let audit = audit(&snapshot, &items);
    let original = publish_audit(&mut db, &audit, &items).unwrap();
    assert!(db
        .publish_research_checked(PERSONA, &audit, &items, &"f".repeat(64), &|| Ok(()))
        .is_err());
    let mut changed = audit.clone();
    changed.receipt.coverage.limitations.push("Other".into());
    assert!(publish_audit(&mut db, &changed, &items).is_err());
    changed = audit.clone();
    changed.receipt.snapshot.id = "other-memory".into();
    assert!(publish_audit(&mut db, &changed, &items).is_err());
    assert!(read(&db, "other-memory").is_err());
    assert_eq!(read_audit(&db).unwrap(), original);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn every_cancel_checkpoint_rolls_back_both_publication_and_bundle() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    let audit = audit(&snapshot, &items);
    let calls = std::cell::Cell::new(0);
    db.publish_research_checked(PERSONA, &audit, &items, &"a".repeat(64), &|| {
        calls.set(calls.get() + 1);
        Ok(())
    })
    .unwrap();
    let checks = calls.get();
    assert!(checks > 5);
    db.connection
        .execute_batch("DELETE FROM memory_snapshots; DELETE FROM memory_items")
        .unwrap();
    for fail_at in 1..=checks {
        calls.set(0);
        let error = db
            .publish_research_checked(PERSONA, &audit, &items, &"a".repeat(64), &|| {
                calls.set(calls.get() + 1);
                if calls.get() == fail_at {
                    Err("Остановлено.".into())
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        assert_eq!(error, "Остановлено.", "checkpoint {fail_at}");
        assert_no_memory_rows(&db.connection);
        assert_eq!(memory_row_count(&db.connection, "memory_publications"), 0);
    }
    publish_audit(&mut db, &audit, &items).unwrap();
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn tampered_publication_hash_counters_versions_and_links_fail_without_repair() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    let audit = audit(&snapshot, &items);
    publish_audit(&mut db, &audit, &items).unwrap();
    db.connection
        .execute_batch(
            "DROP TRIGGER memory_publications_immutable; PRAGMA ignore_check_constraints=ON",
        )
        .unwrap();
    let original = serde_json::to_string(&audit).unwrap();
    for mutation in ["input", "hash", "size", "version", "identity", "large"] {
        db.connection.execute("UPDATE memory_publications SET input_sha256=?4,data_json=?1,payload_bytes=?2,canonical_sha256=?3", params![original, original.len() as i64, sha256_hex(original.as_bytes()), "a".repeat(64)]).unwrap();
        match mutation {
            "input" => {
                db.connection
                    .execute(
                        "UPDATE memory_publications SET input_sha256=?1",
                        ["f".repeat(64)],
                    )
                    .unwrap();
            }
            "hash" => {
                db.connection
                    .execute(
                        "UPDATE memory_publications SET canonical_sha256=?1",
                        ["f".repeat(64)],
                    )
                    .unwrap();
            }
            "size" => {
                db.connection
                    .execute("UPDATE memory_publications SET payload_bytes=1", [])
                    .unwrap();
            }
            "large" => {
                db.connection.execute("UPDATE memory_publications SET payload_bytes=16777217,data_json=zeroblob(16777217)", []).unwrap();
            }
            _ => {
                let mut value = serde_json::to_value(&audit).unwrap();
                if mutation == "version" {
                    value["schemaVersion"] = 9.into();
                } else {
                    value["receipt"]["id"] = "foreign".into();
                }
                let json = value.to_string();
                db.connection.execute("UPDATE memory_publications SET data_json=?1,payload_bytes=?2,canonical_sha256=?3", params![json,json.len() as i64,sha256_hex(json.as_bytes())]).unwrap();
            }
        }
        assert_eq!(read_audit(&db).unwrap_err(), INVALID, "{mutation}");
    }
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legacy_bundle_stays_readable_and_publication_read_preserves_caller_transaction() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    publish(&mut db, &snapshot, &items).unwrap();
    assert!(read_audit(&db).is_err());
    assert_eq!(read(&db, &snapshot.id).unwrap().items, items);
    let audit = audit(&snapshot, &items);
    publish_audit(&mut db, &audit, &items).unwrap();
    db.connection.execute_batch("SAVEPOINT outer_read").unwrap();
    read_audit(&db).unwrap();
    assert!(!db.connection.is_autocommit());
    assert!(db
        .read_research_publication_checked("publication:1", PERSONA, &|| Err("Отмена".into()))
        .is_err());
    assert!(!db.connection.is_autocommit());
    db.connection
        .execute_batch("ROLLBACK TO outer_read; RELEASE outer_read")
        .unwrap();
    read_audit(&db).unwrap();
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn publication_cannot_change_item_coverage_policy_time_or_bridge_separate_excerpts() {
    let (mut db, dir) = setup();
    let (_, _, snapshot, items) = single_bundle(&mut db, "corpus-a");
    let audit = audit(&snapshot, &items);
    for change in ["coverage", "policy", "time", "outside"] {
        let mut changed = items.clone();
        let mut record = audit.clone();
        match change {
            "coverage" => record.receipt.coverage.used_documents = 0,
            "policy" => changed[0].policy_version = "foreign-policy".into(),
            "time" => changed[0].observed_at = "2026-11-09T00:00:00Z".into(),
            _ => changed[0].source_spans[0].end += 1,
        }
        assert!(
            publish_audit(&mut db, &record, &changed).is_err(),
            "{change}"
        );
        assert_no_memory_rows(&db.connection);
    }
    let mut split = audit.clone();
    let mut first = split.retrievals[0].hits[0].clone();
    first.spans[0].end = 2;
    let mut second = first.clone();
    second.chunk_id = "chunk:second".into();
    second.spans[0].start = 2;
    second.spans[0].end = 8;
    split.receipt.analyzed_spans = vec![first.spans[0].clone(), second.spans[0].clone()];
    split.retrievals[0].hits = vec![first, second];
    split.retrievals[0].source_bytes = 8;
    let mut selected = items.clone();
    selected[0].source_spans[0].start = 1;
    selected[0].source_spans[0].end = 5;
    assert!(
        publish_audit(&mut db, &split, &selected).is_err(),
        "cannot join separate excerpts"
    );
    assert_no_memory_rows(&db.connection);
    selected[0].source_spans[0].start = 3;
    publish_audit(&mut db, &split, &selected).unwrap();
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
