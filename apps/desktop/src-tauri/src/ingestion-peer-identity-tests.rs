use super::ingestion_fixture::{archive, prepare, prepare_with_version};
use super::*;
use crate::corpus_database::{JobBudget, JobProvider, JobStatus};
use crate::ingestion::{telegram_identity::ArchiveVersion, telegram_worker::run_reader};

fn assert_documents(
    sources: &Sources,
    op: &crate::ingestion::operations::Operation,
    job: &crate::corpus_database::StoredJob,
    version: ArchiveVersion,
    origin: &str,
) {
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(job.state.coverage.loaded_documents, 3);
    assert_eq!(job.state.checkpoint.adapter_version, version.adapter());
    sources
        .with_ingestion_database(op, |db| {
            let snapshot = db.snapshot(job.state.published_snapshot_id.as_deref().unwrap())?;
            assert_eq!(snapshot.documents.len(), 3);
            assert!(!snapshot.records.is_empty());
            let mut batch_documents = 0;
            for entry in &snapshot.records {
                let record = db.read(&entry.id)?.unwrap();
                // Snapshot dependencies also include preview/grants with no originals.
                if record.references.is_empty() {
                    continue;
                }
                batch_documents += record.references.len();
                assert_eq!(record.input.kind, "import");
                assert_eq!(record.generation, entry.generation);
                let metadata: serde_json::Value =
                    serde_json::from_str(&record.input.state_json).unwrap();
                assert_eq!(metadata["codec"], version.batch_codec());
            }
            assert_eq!(batch_documents, snapshot.documents.len());
            for reference in &snapshot.documents {
                let stored = db.revision(reference)?;
                let decoded =
                    crate::ingestion::telegram_document::decode_revision(version, origin, &stored)?;
                assert_eq!(decoded.id, reference.document_id);
                let value: serde_json::Value = serde_json::from_str(&stored.document_json).unwrap();
                assert_eq!(value["schemaVersion"], version.document_version());
                assert_eq!(stored.provenance.adapter_version, version.adapter());
                assert_eq!(
                    value["resourceId"],
                    if version == ArchiveVersion::V1 {
                        "telegram:chat:1"
                    } else {
                        "telegram:peer:user:1"
                    }
                );
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn peer_class_new_jobs_use_matching_v2_preview_checkpoint_and_documents() {
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
    assert_documents(&sources, &op, &job, ArchiveVersion::V2, &origin);
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn peer_class_legacy_checkpoint_resumes_without_rekeying_saved_v1_documents() {
    let (sources, dir) = local();
    let raw = archive();
    let budget = JobBudget {
        max_input_bytes: 1 << 20,
        max_expanded_bytes: 1 << 20,
        max_documents: 100,
        max_batch_documents: 1,
        max_batch_bytes: 65536,
        max_duration_ms: 60000,
    };
    let (inputs, _, origin) =
        prepare_with_version(&sources, &dir, &raw, budget, ArchiveVersion::V1);
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    let end = raw.find("},{").unwrap() + 1;
    assert!(run_reader(
        &sources,
        &op,
        "job",
        &raw.as_bytes()[..end],
        &origin,
        || 1001
    )
    .is_err());
    let (prior, first_refs) = sources
        .with_ingestion_database(&op, |db| {
            let job = db.job("job")?.unwrap();
            assert_eq!(job.state.coverage.loaded_documents, 1);
            let mut refs = Vec::new();
            for id in db.ids_in_namespace(crate::corpus_database::RecordNamespace::Ingestion)? {
                let record = db.read(&id)?.unwrap();
                if record.input.kind == "import" {
                    refs.extend(record.references);
                }
            }
            assert_eq!(refs.len(), 1);
            db.pause_job("job")?;
            Ok((job, refs))
        })
        .unwrap();
    let connections = sources.0.connections.clone();
    drop(op);
    drop(inputs);
    drop(sources);
    let sources = Sources::new(dir.clone(), connections);
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    let job = run_reader(&sources, &op, "job", raw.as_bytes(), &origin, || 1002).unwrap();
    assert!(job.state.checkpoint.committed_batch > prior.state.checkpoint.committed_batch);
    assert_documents(&sources, &op, &job, ArchiveVersion::V1, &origin);
    sources
        .with_ingestion_database(&op, |db| {
            let snapshot = db.snapshot(job.state.published_snapshot_id.as_deref().unwrap())?;
            assert!(snapshot.documents.contains(&first_refs[0]));
            Ok(())
        })
        .unwrap();
    let replay = run_reader(&sources, &op, "job", raw.as_bytes(), &origin, || 1003).unwrap();
    assert_eq!(replay, job);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn peer_class_mismatched_job_and_preview_versions_fail_before_source_read() {
    use std::io::{self, Read};
    struct Unread;
    impl Read for Unread {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("A mismatched archive version must fail before reading source bytes");
        }
    }
    let (sources, dir) = local();
    let (inputs, _, origin) = prepare(&sources, &dir, &archive());
    let op = sources
        .begin_ingestion_operation("wrong-version", JobProvider::Telegram)
        .unwrap();
    sources
        .with_ingestion_database(&op, |db| {
            let job = db.job("job")?.unwrap();
            db.create_job(&crate::corpus_database::JobInput {
                id: "wrong-version".into(),
                adapter_version: ArchiveVersion::V1.adapter().into(),
                selection: job.state.selection,
                budget: job.state.budget,
            })?;
            Ok(())
        })
        .unwrap();
    assert!(run_reader(&sources, &op, "wrong-version", Unread, &origin, || 1001).is_err());
    sources
        .with_ingestion_database(&op, |db| {
            let job = db.job("wrong-version")?.unwrap();
            assert_eq!(job.status, JobStatus::Queued);
            assert_eq!(job.state.coverage.loaded_documents, 0);
            Ok(())
        })
        .unwrap();
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
