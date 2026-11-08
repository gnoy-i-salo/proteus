use super::ingestion_fixture::{archive, prepare as acquire};
use super::*;
use crate::corpus_database::{
    episodes::{ChunkPolicy, EpisodePolicy, EpisodeTokenizer},
    JobProvider,
};
use crate::ingestion::{
    preparation::{prepare, PrepareInput},
    publication::publish,
    telegram_worker::run_reader,
};
struct Tokenizer;
impl EpisodeTokenizer for Tokenizer {
    fn version(&self) -> &str {
        "syntheticNativePreparation:1"
    }
    fn count(&self, text: &str) -> Result<usize, String> {
        Ok(text.chars().count() + 2)
    }
}
fn source_archive() -> String {
    archive().replace(
        "\"type\":\"message\"",
        "\"type\":\"message\",\"date_unixtime\":\"1704067200\"",
    )
}
fn policies() -> (EpisodePolicy, ChunkPolicy) {
    (
        EpisodePolicy {
            time_gap_ms: 1000,
            holdout_basis_points: 0,
            seed: "fixture".into(),
        },
        ChunkPolicy {
            max_tokens: 16,
            max_bytes: 128,
        },
    )
}
#[test]
fn native_prepared_publication_is_separate_replayable_and_survives_restart() {
    let (sources, dir) = local();
    let (inputs, preview_id, _) = acquire(&sources, &dir, &source_archive());
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    inputs
        .read_preview(&preview_id, |file, origin| {
            run_reader(&sources, &op, "job", file, origin, || 1001)
        })
        .unwrap();
    let raw = publish(&sources, &op, "job", 70000).unwrap();
    let (split, chunks) = policies();
    let request = PrepareInput {
        job_id: "job",
        split: &split,
        chunks: &chunks,
        tokenizer: &Tokenizer,
        max_duration_ms: 60000,
    };
    let prepared = prepare(&sources, &op, &request, || 70000).unwrap();
    assert_ne!(prepared.id, raw.id);
    assert_eq!(
        prepared.parent_snapshot_id.as_deref(),
        Some(raw.id.as_str())
    );
    assert_eq!(prepared.document_refs, raw.document_refs);
    assert_eq!(prepared.binding_ids, raw.binding_ids);
    assert_eq!(prepared.coverage, raw.coverage);
    assert!(!prepared.episode_ids.is_empty());
    assert_ne!(prepared.split_version, raw.split_version);
    assert_eq!(
        prepare(&sources, &op, &request, || 70001).unwrap(),
        prepared
    );
    assert_eq!(publish(&sources, &op, "job", 70002).unwrap(), raw);
    sources
        .with_ingestion_database(&op, |db| {
            let stored = db.prepared_snapshot(&prepared.id)?;
            assert_eq!(stored.tokenizer_version, Tokenizer.version());
            assert!(stored.chunk_count > 0);
            for episode in &prepared.episode_ids {
                for chunk in db.prepared_chunks(&prepared.id, episode, 0, 100, 1 << 20)? {
                    let mut original = String::new();
                    for span in &chunk.spans {
                        let reference = crate::corpus_database::RevisionReference {
                            document_id: span.document_id.clone(),
                            revision_id: span.revision_id.clone(),
                        };
                        let revision = db.revision(&reference)?;
                        let value: serde_json::Value =
                            serde_json::from_str(&revision.document_json).unwrap();
                        let text = value["text"].as_str().unwrap();
                        original.push_str(&text[span.start..span.end]);
                    }
                    assert_eq!(chunk.text, original);
                }
            }
            Ok(())
        })
        .unwrap();
    let connections = sources.0.connections.clone();
    drop(op);
    drop(inputs);
    drop(sources);
    let sources = Sources::new(dir.clone(), connections);
    let op = sources
        .begin_ingestion_operation("prepare-again", JobProvider::Telegram)
        .unwrap();
    assert_eq!(
        prepare(&sources, &op, &request, || 70003).unwrap(),
        prepared
    );
    assert_eq!(publish(&sources, &op, "job", 70004).unwrap(), raw);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_prepared_publication_does_not_hold_gate_and_aborts_during_tokenization() {
    use std::cell::Cell;
    struct ActionTokenizer<'a> {
        action: &'a dyn Fn() -> Result<(), String>,
        calls: Cell<usize>,
    }
    impl EpisodeTokenizer for ActionTokenizer<'_> {
        fn version(&self) -> &str {
            "syntheticAction:1"
        }
        fn count(&self, text: &str) -> Result<usize, String> {
            self.calls.set(self.calls.get() + 1);
            if self.calls.get() == 1 {
                (self.action)()?;
            }
            Ok(text.chars().count() + 2)
        }
    }
    for scenario in ["cancel", "revoke", "expire", "budget"] {
        let (sources, dir) = local();
        let (inputs, preview_id, _) = acquire(&sources, &dir, &source_archive());
        let op = sources
            .begin_ingestion_operation("job", JobProvider::Telegram)
            .unwrap();
        let job = inputs
            .read_preview(&preview_id, |file, origin| {
                run_reader(&sources, &op, "job", file, origin, || 1001)
            })
            .unwrap();
        let expiry = 1000 + 86_400_000;
        let started = if scenario == "expire" {
            expiry - 10
        } else {
            70000
        };
        let clock = Cell::new(started);
        let changed = Cell::new(false);
        let action = || {
            // Fail promptly if a regression moves tokenization under the gate.
            {
                let guard = sources
                    .0
                    .gate
                    .try_lock()
                    .map_err(|_| "Tokenization retained the database gate")?;
                drop(guard);
            }
            match scenario {
                "cancel" => sources.cancel_ingestion_operation("job")?,
                "revoke" => sources.with_ingestion_job_state(|db| {
                    db.remove(&job.state.selection.bindings[0].grant_id)?;
                    Ok(())
                })?,
                "expire" => clock.set(expiry),
                _ => clock.set(started + 1000),
            }
            changed.set(true);
            Ok(())
        };
        let (split, chunks) = policies();
        let tokenizer = ActionTokenizer {
            action: &action,
            calls: Cell::new(0),
        };
        let error = prepare(
            &sources,
            &op,
            &PrepareInput {
                job_id: "job",
                split: &split,
                chunks: &chunks,
                tokenizer: &tokenizer,
                max_duration_ms: 1000,
            },
            || clock.get(),
        )
        .err()
        .unwrap();
        assert!(changed.get(), "{scenario}: {error}");
        if scenario != "revoke" {
            assert_eq!(tokenizer.calls.get(), 1, "{scenario}");
        }
        if scenario == "budget" {
            assert_eq!(error, "Превышено время подготовки.");
        }
        if scenario == "expire" {
            assert!(error.contains("истекло"), "{error}");
        }
        let inspect = sources
            .begin_ingestion_operation("inspect", JobProvider::Telegram)
            .unwrap();
        sources
            .with_ingestion_database(&inspect, |db| {
                let id = job.state.published_snapshot_id.as_deref().unwrap();
                if scenario == "revoke" {
                    assert!(db.snapshot(id).is_err());
                    let revoked = db.read(&job.state.selection.bindings[0].grant_id)?.unwrap();
                    assert_eq!(revoked.input.kind, "deleted");
                    assert!(revoked.input.documents.is_empty());
                    assert!(revoked.references.is_empty());
                } else {
                    let raw = db.snapshot(id)?;
                    let probe = db.publish(&raw.corpus_id, "probe-after-abort", &raw.records)?;
                    assert_eq!(probe.generation, raw.generation + 1);
                    assert_eq!(db.snapshot(id)?, raw);
                }
                Ok(())
            })
            .unwrap();
        drop(inspect);
        drop(op);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn native_prepared_publication_versions_policy_but_not_processing_budget() {
    let (sources, dir) = local();
    let (inputs, preview_id, _) = acquire(&sources, &dir, &source_archive());
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    inputs
        .read_preview(&preview_id, |file, origin| {
            run_reader(&sources, &op, "job", file, origin, || 1001)
        })
        .unwrap();
    let (split, chunks) = policies();
    let mut request = PrepareInput {
        job_id: "job",
        split: &split,
        chunks: &chunks,
        tokenizer: &Tokenizer,
        max_duration_ms: 1000,
    };
    let first = prepare(&sources, &op, &request, || 70000).unwrap();
    request.max_duration_ms = 10000;
    assert_eq!(prepare(&sources, &op, &request, || 70001).unwrap(), first);
    let smaller = ChunkPolicy {
        max_tokens: 8,
        max_bytes: 128,
    };
    request.chunks = &smaller;
    let changed = prepare(&sources, &op, &request, || 70002).unwrap();
    assert_ne!(changed.id, first.id);
    assert!(changed.generation > first.generation);
    assert_eq!(changed.parent_snapshot_id, first.parent_snapshot_id);
    assert_eq!(changed.document_refs, first.document_refs);
    for invalid in [0, -1, 3_600_001] {
        request.max_duration_ms = invalid;
        assert!(prepare(&sources, &op, &request, || 70003).is_err());
    }
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bundled_tokenizer_prepares_a_native_import_without_frontend_authored_chunks() {
    use crate::ingestion::preparation_workflow::{execute, PrepareRequest};
    let (sources, dir) = local();
    let (inputs, preview_id, _) = acquire(&sources, &dir, &source_archive());
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    inputs
        .read_preview(&preview_id, |file, origin| {
            run_reader(&sources, &op, "job", file, origin, || 1001)
        })
        .unwrap();
    let request = PrepareRequest {
        schema_version: 1,
        operation_id: "job".into(),
        job_id: "job".into(),
        max_duration_ms: 60000,
    };
    // Exercise Tauri's actual copied resource, not the download cache.
    let executable = std::env::current_exe().unwrap();
    let path = executable
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("models/e5-tokenizer.json");
    let prepared = execute(&sources, &op, &request, &path, || 70000).unwrap();
    assert!(!prepared.episode_ids.is_empty());
    sources
        .with_ingestion_database(&op, |db| {
            let stored = db.prepared_snapshot(&prepared.id)?;
            assert!(stored
                .tokenizer_version
                .starts_with("e5Tokenizer:1:614241f622f53c4eeff9890bdc4f31cfecc418b3:"));
            assert!(stored
                .tokenizer_version
                .contains("0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39"));
            Ok(())
        })
        .unwrap();
    assert!(execute(&sources, &op, &request, &dir.join("missing"), || 70000).is_err());
    let clock = std::cell::Cell::new(70000);
    let short = PrepareRequest {
        max_duration_ms: 1,
        ..request
    };
    assert_eq!(
        execute(&sources, &op, &short, &path, || {
            let now = clock.get();
            clock.set(now + 1);
            now
        })
        .err()
        .unwrap(),
        "Превышено время подготовки."
    );
    let foreign = PrepareRequest {
        operation_id: "another".into(),
        ..short
    };
    assert_eq!(
        execute(&sources, &op, &foreign, &path, || 70000)
            .err()
            .unwrap(),
        "Запрос относится к другой операции подготовки."
    );

    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
