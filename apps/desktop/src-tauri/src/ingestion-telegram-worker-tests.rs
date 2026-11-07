use super::*;
use crate::corpus_database::{
    JobBudget, JobInput, JobPeriod, JobProvider, JobRole, JobSelection, JobStatus,
};
use crate::ingestion::{
    grants::{create_binding, BindingRequest},
    preview_workflow::{preview, PreviewRequest},
    telegram_worker::run_reader,
    SelectedInputs,
};
use std::io::{self, Read};
fn archive() -> String {
    r#"{"name":"Чат","type":"personal_chat","id":1,"messages":[{"id":1,"type":"message","from_id":"user1","text":"ok"},{"id":2,"type":"message","from_id":"user1","text":"ok"},{"id":3,"type":"message","from_id":"user2","text":"  контекст\r\n"},{"id":4,"type":"message","from_id":"outsider","text":"excluded"}]}"#.into()
}
fn prepare(
    sources: &Sources,
    dir: &std::path::Path,
    raw: &str,
) -> (SelectedInputs, String, String) {
    let budget = JobBudget {
        max_input_bytes: 1 << 20,
        max_expanded_bytes: 1 << 20,
        max_documents: 100,
        max_batch_documents: 1,
        max_batch_bytes: 65536,
        max_duration_ms: 60000,
    };
    prepare_with_budget(sources, dir, raw, budget)
}
fn prepare_with_budget(
    sources: &Sources,
    dir: &std::path::Path,
    raw: &str,
    budget: JobBudget,
) -> (SelectedInputs, String, String) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("archive.json");
    std::fs::write(&path, raw).unwrap();
    let inputs = SelectedInputs::default();
    inputs.insert("selected".into(), &path).unwrap();
    let op = sources
        .begin_ingestion_operation("setup", JobProvider::Telegram)
        .unwrap();
    let preview = preview(
        sources,
        &inputs,
        &op,
        PreviewRequest {
            operation_id: "setup".into(),
            source_handle: "selected".into(),
            budget: budget.clone(),
        },
    )
    .unwrap();
    sources
        .with_ingestion_database(&op, |db| {
            let binding = create_binding(
                db,
                BindingRequest {
                    preview_id: preview.id.clone(),
                    resource_id: preview.resources[0].id.clone(),
                    persona_id: "personA".into(),
                    role: JobRole::Own,
                    target_author_id: Some("user1".into()),
                    allowed_context_author_ids: vec!["user2".into()],
                    allow_unknown_context: false,
                    period: JobPeriod {
                        from: None,
                        to: None,
                        include_undated: true,
                    },
                    allow_model_processing: false,
                    allow_export: false,
                },
                1000,
            )?;
            db.create_job(&JobInput {
                id: "job".into(),
                adapter_version: "telegramDesktopJson:1".into(),
                budget,
                selection: JobSelection {
                    preview_id: preview.id.clone(),
                    resource_ids: vec![binding.resource_id.clone()],
                    bindings: vec![binding],
                },
            })?;
            Ok(())
        })
        .unwrap();
    let origin = inputs
        .read_scoped("selected", |_, origin| Ok(origin.to_string()))
        .unwrap();
    (inputs, preview.id, origin)
}
#[test]
fn archive_worker_publishes_only_selected_authors_in_bounded_batches_with_exact_originals() {
    let (sources, dir) = local();
    let raw = archive();
    let (inputs, preview_id, origin) = prepare(&sources, &dir, &raw);
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    let job = inputs
        .read_preview(&preview_id, |file, origin| {
            run_reader(&sources, &op, "job", file, origin, || 1001)
        })
        .unwrap();
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(job.state.coverage.loaded_documents, 3);
    assert_eq!(job.state.coverage.indexed_documents, 0);
    assert_eq!(job.state.coverage.analyzed_documents, 0);
    assert_eq!(job.state.coverage.used_documents, 0);
    assert!(job.state.checkpoint.committed_batch >= 3);
    let snapshot_id = job.state.published_snapshot_id.as_deref().unwrap();
    sources
        .with_ingestion_database(&op, |db| {
            let snapshot = db.snapshot(snapshot_id)?;
            assert_eq!(snapshot.documents.len(), 3);
            let mut docs = snapshot
                .documents
                .iter()
                .map(|r| db.revision(r).unwrap())
                .collect::<Vec<_>>();
            docs.sort_by_key(|d| {
                serde_json::from_str::<serde_json::Value>(&d.document_json).unwrap()["originalId"]
                    .as_str()
                    .unwrap()
                    .to_string()
            });
            let first: serde_json::Value = serde_json::from_str(&docs[0].document_json).unwrap();
            assert_eq!(first["text"], "ok");
            assert_eq!(
                first["rawJson"],
                r#"{"id":1,"type":"message","from_id":"user1","text":"ok"}"#
            );
            assert_ne!(docs[0].reference.document_id, docs[1].reference.document_id);
            assert_eq!(docs[0].provenance.original_object_id.as_deref(), Some("1"));
            Ok(())
        })
        .unwrap();
    let retry = run_reader(&sources, &op, "job", io::empty(), &origin, || 1002).unwrap();
    assert_eq!(retry, job);
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
struct Interrupted<'a> {
    bytes: &'a [u8],
    remaining: usize,
}
impl Read for Interrupted<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other("simulated interruption"));
        }
        let n = out.len().min(self.remaining).min(16);
        let count = self.bytes.read(&mut out[..n])?;
        self.remaining -= count;
        Ok(count)
    }
}
#[test]
fn archive_worker_restarts_from_a_durable_checkpoint_without_duplicate_replicas() {
    let (sources, dir) = local();
    let raw = archive();
    let (inputs, _, origin) = prepare(&sources, &dir, &raw);
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    let stop = raw.find(r#"{"id":3"#).unwrap();
    assert!(run_reader(
        &sources,
        &op,
        "job",
        Interrupted {
            bytes: raw.as_bytes(),
            remaining: stop
        },
        &origin,
        || 1001
    )
    .is_err());
    sources
        .with_ingestion_database(&op, |db| {
            let job = db.job("job")?.unwrap();
            assert_eq!(job.state.coverage.loaded_documents, 2);
            assert!(job.state.published_snapshot_id.is_none());
            db.recover_interrupted_jobs()?;
            Ok(())
        })
        .unwrap();
    let connections = sources.0.connections.clone();
    drop(op);
    drop(inputs);
    drop(sources);
    let restored = Sources::new(dir.clone(), connections);
    let op = restored
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    let job = run_reader(&restored, &op, "job", raw.as_bytes(), &origin, || 1002).unwrap();
    assert_eq!(job.state.coverage.loaded_documents, 3);
    restored
        .with_ingestion_database(&op, |db| {
            assert_eq!(
                db.snapshot(job.state.published_snapshot_id.as_deref().unwrap())?
                    .documents
                    .len(),
                3
            );
            Ok(())
        })
        .unwrap();
    drop(op);
    drop(restored);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn edited_archive_and_malformed_tail_never_publish_a_partial_snapshot() {
    for edited in [true, false] {
        let (sources, dir) = local();
        let raw = archive();
        let (inputs, _, origin) = prepare(&sources, &dir, &raw);
        let op = sources
            .begin_ingestion_operation("job", JobProvider::Telegram)
            .unwrap();
        let changed = if edited {
            raw.replace("ok", "changed")
        } else {
            format!("{raw} garbage")
        };
        assert!(run_reader(&sources, &op, "job", changed.as_bytes(), &origin, || 1001).is_err());
        sources
            .with_ingestion_database(&op, |db| {
                let job = db.job("job")?.unwrap();
                assert!(job.state.published_snapshot_id.is_none());
                assert!(!job.state.coverage.complete);
                Ok(())
            })
            .unwrap();
        drop(op);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
struct Cancelled<'a> {
    input: Interrupted<'a>,
    sources: &'a Sources,
}
impl Read for Cancelled<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.input.remaining == 0 {
            self.sources.cancel_ingestion_operation("job").unwrap();
        }
        self.input.read(out)
    }
}
#[test]
fn cancellation_after_committed_batches_cannot_publish_even_with_complete_previous_metadata() {
    let (sources, dir) = local();
    let raw = archive();
    let (inputs, _, origin) = prepare(&sources, &dir, &raw);
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    let stop = raw.find(r#"{"id":3"#).unwrap();
    assert!(run_reader(
        &sources,
        &op,
        "job",
        Cancelled {
            input: Interrupted {
                bytes: raw.as_bytes(),
                remaining: stop
            },
            sources: &sources
        },
        &origin,
        || 1001
    )
    .is_err());
    drop(op);
    let inspect = sources
        .begin_ingestion_operation("inspect", JobProvider::Telegram)
        .unwrap();
    sources
        .with_ingestion_database(&inspect, |db| {
            let job = db.job("job")?.unwrap();
            assert_eq!(job.state.coverage.loaded_documents, 2);
            assert!(job.state.published_snapshot_id.is_none());
            assert!(!job.state.coverage.complete);
            Ok(())
        })
        .unwrap();
    drop(inspect);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn a_second_import_of_the_same_originals_reuses_document_revisions() {
    let (sources, dir) = local();
    let raw = archive();
    let (inputs, _, origin) = prepare(&sources, &dir, &raw);
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    let first = run_reader(&sources, &op, "job", raw.as_bytes(), &origin, || 1001).unwrap();
    sources
        .with_ingestion_database(&op, |db| {
            db.create_job(&JobInput {
                id: "next-job".into(),
                adapter_version: first.state.checkpoint.adapter_version.clone(),
                selection: first.state.selection.clone(),
                budget: first.state.budget.clone(),
            })?;
            Ok(())
        })
        .unwrap();
    let second = run_reader(&sources, &op, "next-job", raw.as_bytes(), &origin, || 1002).unwrap();
    sources
        .with_ingestion_database(&op, |db| {
            assert_eq!(
                db.snapshot(first.state.published_snapshot_id.as_deref().unwrap())?
                    .documents,
                db.snapshot(second.state.published_snapshot_id.as_deref().unwrap())?
                    .documents
            );
            Ok(())
        })
        .unwrap();
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

struct EndClock<'a> {
    bytes: &'a [u8],
    clock: &'a std::cell::Cell<i64>,
}
impl Read for EndClock<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let count = self.bytes.read(out)?;
        if count == 0 {
            self.clock.set(61001);
        }
        Ok(count)
    }
}
#[test]
fn overall_deadline_is_checked_again_before_publication_after_end_of_input() {
    let (sources, dir) = local();
    let raw = archive();
    let (inputs, _, origin) = prepare(&sources, &dir, &raw);
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    let clock = std::cell::Cell::new(1001);
    assert!(run_reader(
        &sources,
        &op,
        "job",
        EndClock {
            bytes: raw.as_bytes(),
            clock: &clock
        },
        &origin,
        || clock.get()
    )
    .is_err());
    sources
        .with_ingestion_database(&op, |db| {
            assert!(db
                .job("job")?
                .unwrap()
                .state
                .published_snapshot_id
                .is_none());
            Ok(())
        })
        .unwrap();
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn resume_rejects_a_prefix_staged_from_a_changed_archive_even_when_retry_matches_preview() {
    let (sources, dir) = local();
    let raw = archive();
    let (inputs, _, origin) = prepare(&sources, &dir, &raw);
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    let changed = raw.replace("ok", "changed");
    assert!(run_reader(&sources, &op, "job", changed.as_bytes(), &origin, || 1001).is_err());
    assert!(
        run_reader(&sources, &op, "job", raw.as_bytes(), &origin, || 1002).is_err(),
        "verified tail must not publish a prefix from a different input version"
    );
    sources
        .with_ingestion_database(&op, |db| {
            assert!(db
                .job("job")?
                .unwrap()
                .state
                .published_snapshot_id
                .is_none());
            Ok(())
        })
        .unwrap();
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_archive_worker_imports_above_old_document_and_byte_limits() {
    let (sources, dir) = local();
    let mut raw = String::from(r#"{"name":"Large","type":"personal_chat","id":1,"messages":["#);
    let text = format!("{} é😀\r\n", "x".repeat(1800));
    for id in 1..=10400 {
        if id > 1 {
            raw.push(',');
        }
        raw.push_str(&serde_json::json!({"id":id,"type":"message","from_id":if id % 2 == 0 {"user1"} else {"user2"},"text":text}).to_string());
    }
    raw.push_str("]}");
    assert!(raw.len() > 16 * 1024 * 1024);
    let budget = JobBudget {
        max_input_bytes: raw.len() as i64,
        max_expanded_bytes: 128 * 1024 * 1024,
        max_documents: 10400,
        max_batch_documents: 64,
        max_batch_bytes: 512 * 1024,
        max_duration_ms: 300000,
    };
    let (inputs, preview_id, _) = prepare_with_budget(&sources, &dir, &raw, budget);
    drop(raw);
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    let job = inputs
        .read_preview(&preview_id, |file, origin| {
            run_reader(&sources, &op, "job", file, origin, || 1001)
        })
        .unwrap();
    assert_eq!(job.state.coverage.loaded_documents, 10400);
    assert!(job.state.coverage.loaded_bytes > 16 * 1024 * 1024);
    assert_eq!(job.state.coverage.indexed_documents, 0);
    assert_eq!(job.state.coverage.analyzed_documents, 0);
    assert_eq!(job.state.coverage.used_documents, 0);
    assert!(job.state.checkpoint.committed_batch > 160);
    assert!(job.state.published_snapshot_id.is_some());
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
