use super::*;
use crate::ingestion::search_query::{query, QueryInput};
fn input(snapshot: &crate::ingestion::publication::SnapshotView) -> QueryInput<'_> {
    QueryInput {
        job_id: "job",
        snapshot_id: &snapshot.id,
        persona_id: "personA",
        query_text: "контекст",
        period: SearchPeriod {
            from: None,
            to: None,
            include_undated: false,
        },
        limit: 10,
        offset: 0,
        expected_index_generation: None,
        max_duration_ms: 60000,
    }
}
fn indexed(
    sources: &Sources,
    dir: &std::path::Path,
    holdout: u16,
) -> (
    crate::ingestion::operations::Operation,
    crate::ingestion::publication::SnapshotView,
    crate::corpus_database::search::SearchIndexReceipt,
) {
    let (op, prepared) = prepared(sources, dir, holdout);
    let receipt = rebuild(
        sources,
        &op,
        &IndexInput {
            job_id: "job",
            snapshot_id: &prepared.id,
            max_duration_ms: 60000,
        },
        || 70001,
    )
    .unwrap();
    (op, prepared, receipt)
}
#[test]
fn native_query_derives_scope_from_the_job_and_preserves_exact_references_without_model_permission()
{
    let (sources, dir) = local();
    let (op, snapshot, receipt) = indexed(&sources, &dir, 0);
    let mut request = input(&snapshot);
    let response = query(&sources, &op, &request, || 70002).unwrap();
    assert_eq!(response.index_generation, receipt.index_generation);
    assert_eq!(response.hits.len(), 1);
    let hit = &response.hits[0];
    sources
        .with_ingestion_database(&op, |db| {
            let job = db.job("job")?.unwrap();
            let binding = &job.state.selection.bindings[0];
            assert_eq!(hit.binding_id, binding.id);
            assert_eq!(hit.grant_id, binding.grant_id);
            assert_eq!(hit.persona_id, binding.persona_id);
            assert!(crate::ingestion::grants::authorize_binding(
                db,
                binding,
                crate::ingestion::grants::GrantUse::Model,
                70002
            )
            .is_err());
            let revision = db.revision(&crate::corpus_database::RevisionReference {
                document_id: hit.document_id.clone(),
                revision_id: hit.revision_id.clone(),
            })?;
            let value: serde_json::Value = serde_json::from_str(&revision.document_json).unwrap();
            assert_eq!(
                &value["text"].as_str().unwrap()[hit.span_start as usize..hit.span_end as usize],
                "  контекст\r\n"
            );
            Ok(())
        })
        .unwrap();
    request.query_text = "excluded";
    assert!(query(&sources, &op, &request, || 70002)
        .unwrap()
        .hits
        .is_empty());
    request.query_text = "контекст";
    request.period.from = Some(1704067200001);
    assert!(query(&sources, &op, &request, || 70002)
        .unwrap()
        .hits
        .is_empty());
    request.persona_id = "personB";
    assert!(query(&sources, &op, &request, || 70002).is_err());
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn native_query_refuses_expired_revoked_and_raw_snapshot_access() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let request = input(&snapshot);
    assert!(query(&sources, &op, &request, || 172801000).is_err());
    let raw = QueryInput {
        snapshot_id: snapshot.parent_snapshot_id.as_deref().unwrap(),
        ..input(&snapshot)
    };
    assert!(query(&sources, &op, &raw, || 70002).is_err());
    sources
        .with_ingestion_database(&op, |db| {
            let id = db.job("job")?.unwrap().state.selection.bindings[0]
                .grant_id
                .clone();
            db.remove(&id)
        })
        .unwrap();
    assert!(query(&sources, &op, &request, || 70002).is_err());
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn native_query_requires_a_generation_for_later_pages_and_rejects_rebuilt_or_wrong_policy_indexes()
{
    let (sources, dir) = local();
    let (op, snapshot, receipt) = indexed(&sources, &dir, 0);
    let mut request = input(&snapshot);
    request.offset = 1;
    assert!(query(&sources, &op, &request, || 70002).is_err());
    request.expected_index_generation = Some(receipt.index_generation);
    assert!(query(&sources, &op, &request, || 70002)
        .unwrap()
        .hits
        .is_empty());
    rebuild(
        &sources,
        &op,
        &IndexInput {
            job_id: "job",
            snapshot_id: &snapshot.id,
            max_duration_ms: 60000,
        },
        || 70002,
    )
    .unwrap();
    assert!(query(&sources, &op, &request, || 70003).is_err());
    sources
        .with_ingestion_database(&op, |db| {
            let stage = crate::corpus_database::search::SearchStageInput {
                stage_id: "wrong-policy".into(),
                snapshot_id: snapshot.id.clone(),
                snapshot_generation: snapshot.generation,
                policy_fingerprint: "other-split-or-tokenizer".into(),
            };
            db.begin_search_stage(&stage)?;
            let mut cursor = db.prepared_cursor(&snapshot.id, &|| Ok(()))?;
            let bindings = db.job("job")?.unwrap().state.selection.bindings;
            let mut hashes = Vec::new();
            while !cursor.is_complete() {
                for entry in db.prepared_page(&mut cursor, 100, 64 * 1024 * 1024, &|| Ok(()))? {
                    let binding = bindings
                        .iter()
                        .find(|b| b.id == entry.episode.binding_id)
                        .unwrap();
                    let span = &entry.chunk.spans[0];
                    let chunk = crate::corpus_database::search::SearchChunkInput {
                        stage_id: stage.stage_id.clone(),
                        chunk_id: entry.chunk.id.clone(),
                        snapshot_id: snapshot.id.clone(),
                        snapshot_generation: snapshot.generation,
                        partition: "reference".into(),
                        text: entry.chunk.text,
                        document_id: span.document_id.clone(),
                        revision_id: span.revision_id.clone(),
                        span_start: span.start as i64,
                        span_end: span.end as i64,
                        persona_id: binding.persona_id.clone(),
                        grant_id: binding.grant_id.clone(),
                        binding_id: binding.id.clone(),
                        record_id: entry.source_record.id,
                        record_generation: entry.source_record.generation,
                        period_from: Some(1704067200000),
                        period_to: Some(1704067200000),
                        conversation_id: Some(entry.episode.conversation_id.clone()),
                        thread_id: entry.episode.thread_id.clone(),
                    };
                    db.stage_search_chunk(&chunk)?;
                    hashes.push((
                        chunk.chunk_id.clone(),
                        crate::corpus_database::search::chunk_content_hash(&chunk),
                    ));
                }
            }
            let foreign = db.commit_search_stage(
                &stage.stage_id,
                &crate::corpus_database::search::content_manifest(&hashes),
                &|| Ok(()),
            )?;
            assert_eq!(foreign.chunk_count, receipt.chunk_count);
            assert_ne!(foreign.policy_fingerprint, receipt.policy_fingerprint);
            Ok(())
        })
        .unwrap();
    assert!(query(&sources, &op, &input(&snapshot), || 70003).is_err());
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn native_query_empty_reference_is_truthful_and_unpublished_index_is_not_empty_success() {
    let (sources, dir) = local();
    let (op, snapshot) = prepared(&sources, &dir, 10000);
    assert!(query(&sources, &op, &input(&snapshot), || 70001).is_err());
    let receipt = rebuild(
        &sources,
        &op,
        &IndexInput {
            job_id: "job",
            snapshot_id: &snapshot.id,
            max_duration_ms: 60000,
        },
        || 70001,
    )
    .unwrap();
    let response = query(&sources, &op, &input(&snapshot), || 70002).unwrap();
    assert_eq!(receipt.chunk_count, 0);
    assert!(response.hits.is_empty());
    assert!(!response.has_more);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn native_query_deadline_and_cancellation_discard_results_before_return() {
    use std::cell::Cell;
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let request = input(&snapshot);
    let calls = Cell::new(0);
    query(&sources, &op, &request, || {
        calls.set(calls.get() + 1);
        70002
    })
    .unwrap();
    let total = calls.get();
    for stop in [2, (total / 2).max(2), total.max(2)] {
        calls.set(0);
        assert!(
            query(&sources, &op, &request, || {
                calls.set(calls.get() + 1);
                if calls.get() >= stop {
                    130003
                } else {
                    70002
                }
            })
            .is_err(),
            "deadline checkpoint {stop}/{total}"
        );
    }
    calls.set(0);
    assert!(query(&sources, &op, &request, || {
        calls.set(calls.get() + 1);
        if calls.get() == total {
            sources.cancel_ingestion_operation("job").unwrap();
        }
        70002
    })
    .is_err());
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_query_rejects_invalid_parameters_before_any_clock_or_database_work() {
    use std::cell::Cell;
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let invalid = [
        QueryInput {
            persona_id: "",
            ..input(&snapshot)
        },
        QueryInput {
            snapshot_id: "\n",
            ..input(&snapshot)
        },
        QueryInput {
            query_text: "",
            ..input(&snapshot)
        },
        QueryInput {
            limit: 0,
            ..input(&snapshot)
        },
        QueryInput {
            offset: 10001,
            ..input(&snapshot)
        },
        QueryInput {
            max_duration_ms: 0,
            ..input(&snapshot)
        },
        QueryInput {
            expected_index_generation: Some(0),
            ..input(&snapshot)
        },
        QueryInput {
            period: SearchPeriod {
                from: Some(2),
                to: Some(1),
                include_undated: false,
            },
            ..input(&snapshot)
        },
        QueryInput {
            period: SearchPeriod {
                from: Some(9_007_199_254_740_992),
                to: None,
                include_undated: false,
            },
            ..input(&snapshot)
        },
    ];
    for request in invalid {
        let calls = Cell::new(0);
        assert!(query(&sources, &op, &request, || {
            calls.set(calls.get() + 1);
            70002
        })
        .is_err());
        assert_eq!(calls.get(), 0, "invalid input reached the query clock");
    }
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_query_rechecks_source_deletion_after_the_pinned_search_before_returning_hits() {
    use std::cell::{Cell, RefCell};
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let request = input(&snapshot);
    let calls = Cell::new(0);
    query(&sources, &op, &request, || {
        calls.set(calls.get() + 1);
        70002
    })
    .unwrap();
    let total = calls.get();
    let record = sources
        .with_ingestion_database(&op, |db| {
            let mut cursor = db.prepared_cursor(&snapshot.id, &|| Ok(()))?;
            let entry = db
                .prepared_page(&mut cursor, 1, 64 * 1024 * 1024, &|| Ok(()))?
                .remove(0);
            let source = db.read(&entry.source_record.id)?.unwrap();
            assert!(
                !source.input.documents.is_empty(),
                "delete source data, not a grant/preview authority"
            );
            Ok(entry.source_record.id)
        })
        .unwrap();
    // Only the test's in-memory secret store is used; no account/CLI secrets.
    let other = RefCell::new(
        crate::corpus_database::CorpusDatabase::open_with_secrets(
            &dir.join("metadata.db"),
            sources.0.connections.secrets().as_ref(),
        )
        .unwrap(),
    );
    calls.set(0);
    let removed = Cell::new(false);
    let result = query(&sources, &op, &request, || {
        calls.set(calls.get() + 1);
        if calls.get() == total - 1 {
            other.borrow_mut().remove(&record).unwrap();
            removed.set(true);
        }
        70002
    });
    assert!(removed.get());
    assert!(
        result.is_err(),
        "deleted source escaped the final authorization boundary: {result:?}"
    );
    drop(other);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[path = "ingestion-vectors-tests.rs"]
mod vector_tests;
