use super::ingestion_fixture::{archive, prepare as acquire};
use super::*;
use crate::corpus_database::{
    episodes::{ChunkPolicy, EpisodePolicy, EpisodeTokenizer},
    search::{SearchPeriod, SearchRequest, SearchScope},
    JobProvider,
};
use crate::ingestion::{
    preparation::{prepare, PrepareInput},
    search::{rebuild, IndexInput},
    telegram_worker::run_reader,
};
struct Tokenizer;
impl EpisodeTokenizer for Tokenizer {
    fn version(&self) -> &str {
        "syntheticSearch:1"
    }
    fn count(&self, text: &str) -> Result<usize, String> {
        Ok(text.chars().count() + 2)
    }
}
fn prepared(
    sources: &Sources,
    dir: &std::path::Path,
    holdout: u16,
) -> (
    crate::ingestion::operations::Operation,
    crate::ingestion::publication::SnapshotView,
) {
    let raw = archive().replace(
        "\"type\":\"message\"",
        "\"type\":\"message\",\"date_unixtime\":\"1704067200\"",
    );
    let (inputs, preview, _) = acquire(sources, dir, &raw);
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    inputs
        .read_preview(&preview, |file, origin| {
            run_reader(sources, &op, "job", file, origin, || 1001)
        })
        .unwrap();
    let snapshot = prepare(
        sources,
        &op,
        &PrepareInput {
            job_id: "job",
            split: &EpisodePolicy {
                time_gap_ms: 1000,
                holdout_basis_points: holdout,
                seed: "fixture".into(),
            },
            chunks: &ChunkPolicy {
                max_tokens: 512,
                max_bytes: 4096,
            },
            tokenizer: &Tokenizer,
            max_duration_ms: 60000,
        },
        || 70000,
    )
    .unwrap();
    (op, snapshot)
}
#[test]
fn native_index_uses_prepared_reference_chunks_and_exact_original_links() {
    let (sources, dir) = local();
    let (op, prepared) = prepared(&sources, &dir, 0);
    let receipt = rebuild(
        &sources,
        &op,
        &IndexInput {
            job_id: "job",
            snapshot_id: &prepared.id,
            max_duration_ms: 60000,
        },
        || 70001,
    )
    .unwrap();
    assert_eq!(receipt.snapshot_id, prepared.id);
    assert!(receipt.chunk_count > 0);
    sources
        .with_ingestion_database(&op, |db| {
            let job = db.job("job")?.unwrap();
            let binding = &job.state.selection.bindings[0];
            let response = db.search_corpus(&SearchRequest {
                scope: SearchScope {
                    snapshot_id: prepared.id.clone(),
                    snapshot_generation: prepared.generation,
                    persona_id: binding.persona_id.clone(),
                    grant_ids: vec![binding.grant_id.clone()],
                    binding_ids: vec![binding.id.clone()],
                    period: SearchPeriod {
                        from: None,
                        to: None,
                        include_undated: true,
                    },
                },
                query_text: "контекст".into(),
                limit: 10,
                offset: 0,
            })?;
            assert_eq!(response.hits.len(), 1);
            let hit = &response.hits[0];
            let revision = db.revision(&crate::corpus_database::RevisionReference {
                document_id: hit.document_id.clone(),
                revision_id: hit.revision_id.clone(),
            })?;
            let value: serde_json::Value = serde_json::from_str(&revision.document_json).unwrap();
            assert_eq!(
                &value["text"].as_str().unwrap()[hit.span_start as usize..hit.span_end as usize],
                "  контекст\r\n"
            );
            assert_eq!(hit.persona_id, binding.persona_id);
            assert_eq!(hit.period_from, Some(1704067200000));
            assert_eq!(job.state.published_snapshot_id, prepared.parent_snapshot_id);
            Ok(())
        })
        .unwrap();
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_index_empty_reference_and_rebuild_keep_one_generation_and_raw_receipt() {
    let (sources, dir) = local();
    let (op, prepared) = prepared(&sources, &dir, 10000);
    let input = IndexInput {
        job_id: "job",
        snapshot_id: &prepared.id,
        max_duration_ms: 60000,
    };
    let first = rebuild(&sources, &op, &input, || 70001).unwrap();
    let next = rebuild(&sources, &op, &input, || 70002).unwrap();
    assert_eq!(first.chunk_count, 0);
    assert_eq!(next.chunk_count, 0);
    assert_eq!(next.index_generation, first.index_generation + 1);
    assert_eq!(next.policy_fingerprint, first.policy_fingerprint);
    sources
        .with_ingestion_database(&op, |db| {
            assert_eq!(
                db.job("job")?.unwrap().state.published_snapshot_id,
                prepared.parent_snapshot_id
            );
            Ok(())
        })
        .unwrap();
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn native_index_rechecks_current_grants_and_refuses_the_raw_snapshot() {
    for revoked in [false, true] {
        let (sources, dir) = local();
        let (op, prepared) = prepared(&sources, &dir, 0);
        assert!(rebuild(
            &sources,
            &op,
            &IndexInput {
                job_id: "job",
                snapshot_id: prepared.parent_snapshot_id.as_deref().unwrap(),
                max_duration_ms: 60000
            },
            || 70001
        )
        .is_err());
        if revoked {
            sources
                .with_ingestion_database(&op, |db| {
                    let grant = db.job("job")?.unwrap().state.selection.bindings[0]
                        .grant_id
                        .clone();
                    db.remove(&grant)
                })
                .unwrap();
        }
        assert!(rebuild(
            &sources,
            &op,
            &IndexInput {
                job_id: "job",
                snapshot_id: &prepared.id,
                max_duration_ms: 60000
            },
            || if revoked { 70001 } else { 172801000 }
        )
        .is_err());
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn native_index_deadlines_stop_early_and_late_attempts_without_replacing_the_active_generation() {
    use std::cell::Cell;
    let (sources, dir) = local();
    let (op, prepared) = prepared(&sources, &dir, 0);
    let input = IndexInput {
        job_id: "job",
        snapshot_id: &prepared.id,
        max_duration_ms: 60000,
    };
    let calls = Cell::new(0usize);
    let first = rebuild(&sources, &op, &input, || {
        calls.set(calls.get() + 1);
        70001
    })
    .unwrap();
    let total = calls.get();
    assert!(total > 30);
    for stop in [2, total / 2, total - 1] {
        calls.set(0);
        let result = rebuild(&sources, &op, &input, || {
            calls.set(calls.get() + 1);
            if calls.get() >= stop {
                130002
            } else {
                70001
            }
        });
        assert!(result.is_err(), "deadline checkpoint {stop} of {total}");
    }
    let next = rebuild(&sources, &op, &input, || 70002).unwrap();
    assert_eq!(next.index_generation, first.index_generation + 1);
    // Cancellation is independent of a completed acquisition job.
    calls.set(0);
    assert!(rebuild(&sources, &op, &input, || {
        calls.set(calls.get() + 1);
        if calls.get() == total / 2 {
            sources.cancel_ingestion_operation("job").unwrap();
        }
        70002
    })
    .is_err());
    sources
        .with_ingestion_job_state(|db| {
            assert_eq!(
                db.job("job")?.unwrap().status,
                crate::corpus_database::JobStatus::Completed
            );
            Ok(())
        })
        .unwrap();
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_index_rejects_prepared_chunks_with_a_foreign_binding_despite_valid_storage_hashes() {
    let (sources, dir) = local();
    let (op, _) = prepared(&sources, &dir, 0);
    let mut projection =
        crate::ingestion::projection::project(&sources, &op, "job", || 70001).unwrap();
    for doc in &mut projection.documents {
        doc.binding_id = "foreign-binding".into();
    }
    let invalid = sources
        .with_ingestion_database(&op, |db| {
            db.publish_prepared(&crate::corpus_database::preparation::PreparationRequest {
                source_snapshot_id: &projection.source.id,
                publication_id: "foreign-preparation",
                documents: &projection.documents,
                split: &EpisodePolicy {
                    time_gap_ms: 1000,
                    holdout_basis_points: 0,
                    seed: "fixture".into(),
                },
                chunks: &ChunkPolicy {
                    max_tokens: 512,
                    max_bytes: 4096,
                },
                tokenizer: &Tokenizer,
            })
        })
        .unwrap();
    let error = rebuild(
        &sources,
        &op,
        &IndexInput {
            job_id: "job",
            snapshot_id: &invalid.snapshot.id,
            max_duration_ms: 60000,
        },
        || 70002,
    )
    .unwrap_err();
    assert_eq!(error, "Фрагмент не принадлежит разрешённой привязке.");
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_index_checks_snapshot_ownership_before_starting_any_prepared_content_read() {
    use std::cell::Cell;
    let (sources, dir) = local();
    let (op, _) = prepared(&sources, &dir, 0);
    let projection = crate::ingestion::projection::project(&sources, &op, "job", || 70001).unwrap();
    let foreign = sources
        .with_ingestion_database(&op, |db| {
            let raw = db.publish("foreign-corpus", "foreign-raw", &projection.source.records)?;
            db.publish_prepared(&crate::corpus_database::preparation::PreparationRequest {
                source_snapshot_id: &raw.id,
                publication_id: "foreign-prepared",
                documents: &projection.documents,
                split: &EpisodePolicy {
                    time_gap_ms: 1000,
                    holdout_basis_points: 0,
                    seed: "fixture".into(),
                },
                chunks: &ChunkPolicy {
                    max_tokens: 512,
                    max_bytes: 4096,
                },
                tokenizer: &Tokenizer,
            })
        })
        .unwrap();
    let calls = Cell::new(0);
    let error = rebuild(
        &sources,
        &op,
        &IndexInput {
            job_id: "job",
            snapshot_id: &foreign.snapshot.id,
            max_duration_ms: 60000,
        },
        || {
            calls.set(calls.get() + 1);
            // Four clock reads establish budget and current job grants. Entering the
            // prepared-content reader would call its guard and exhaust this budget.
            if calls.get() > 4 {
                130002
            } else {
                70001
            }
        },
    )
    .unwrap_err();
    assert_eq!(error, "Подготовленный снимок не принадлежит задаче.");
    assert_eq!(calls.get(), 4);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[path = "ingestion-search-query-tests.rs"]
mod query_tests;
