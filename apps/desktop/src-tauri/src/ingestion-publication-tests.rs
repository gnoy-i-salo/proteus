use super::ingestion_fixture::{archive, prepare};
use super::*;
use crate::corpus_database::JobProvider;
use crate::ingestion::{publication::publish, telegram_worker::run_reader};

#[test]
fn completed_acquisition_returns_exact_replayable_metadata_without_claiming_preparation() {
    let (sources, dir) = local();
    let (inputs, preview_id, _) = prepare(&sources, &dir, &archive());
    let op = sources
        .begin_ingestion_operation("job", JobProvider::Telegram)
        .unwrap();
    let job = inputs
        .read_preview(&preview_id, |file, origin| {
            run_reader(&sources, &op, "job", file, origin, || 1001)
        })
        .unwrap();
    let view = publish(&sources, &op, "job", 1002).unwrap();
    assert_eq!(view.id, job.state.published_snapshot_id.unwrap());
    assert_eq!(view.coverage, job.state.coverage);
    assert_eq!(
        view.binding_ids,
        job.state
            .selection
            .bindings
            .iter()
            .map(|b| b.id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(view.document_refs.len(), 3);
    assert_eq!(view.generation, 1);
    assert_eq!(view.parent_snapshot_id, None);
    assert_eq!(view.split_version, "unpartitioned:1");
    assert!(view.episode_ids.is_empty());
    assert_eq!(publish(&sources, &op, "job", 1003).unwrap(), view);
    let wire = serde_json::to_value(&view).unwrap();
    assert_eq!(wire.as_object().unwrap().len(), 11);
    assert_eq!(wire["schemaVersion"], 1);
    assert_eq!(
        wire["documentRefs"][0]["revisionId"],
        view.document_refs[0].revision_id
    );
    assert!(wire.get("publicationId").is_none());
    assert!(wire.get("records").is_none());
    assert!(wire.get("state").is_none());
    let connections = sources.0.connections.clone();
    drop(op);
    drop(inputs);
    drop(sources);
    let sources = Sources::new(dir.clone(), connections);
    let op = sources
        .begin_ingestion_operation("read-restarted", JobProvider::Telegram)
        .unwrap();
    assert_eq!(publish(&sources, &op, "job", 1004).unwrap(), view);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn publication_rechecks_current_authority_even_after_a_successful_receipt() {
    for scenario in ["expired", "revoked", "cancelled-operation"] {
        let (sources, dir) = local();
        let (inputs, preview_id, _) = prepare(&sources, &dir, &archive());
        let op = sources
            .begin_ingestion_operation("job", JobProvider::Telegram)
            .unwrap();
        let job = inputs
            .read_preview(&preview_id, |file, origin| {
                run_reader(&sources, &op, "job", file, origin, || 1001)
            })
            .unwrap();
        assert!(publish(&sources, &op, "job", 1002).is_ok());
        let now = match scenario {
            "expired" => 1000 + 86_400_000,
            "revoked" => {
                sources
                    .with_ingestion_database(&op, |db| {
                        db.remove(&job.state.selection.bindings[0].grant_id)?;
                        Ok(())
                    })
                    .unwrap();
                1003
            }
            _ => {
                sources.cancel_ingestion_operation("job").unwrap();
                1003
            }
        };
        assert!(publish(&sources, &op, "job", now).is_err(), "{scenario}");
        drop(op);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn incomplete_and_cancelled_jobs_never_become_published_by_inspection() {
    for cancel in [false, true] {
        let (sources, dir) = local();
        let (inputs, _, _) = prepare(&sources, &dir, &archive());
        let op = sources
            .begin_ingestion_operation("inspect", JobProvider::Telegram)
            .unwrap();
        if cancel {
            sources
                .with_ingestion_database(&op, |db| db.cancel_job("job"))
                .unwrap();
        }
        assert!(publish(&sources, &op, "job", 1001).is_err());
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
}

#[test]
fn completed_job_without_receipt_can_finish_publication_once_after_restart() {
    let (sources, dir) = local();
    let (inputs, preview_id, _) = prepare(&sources, &dir, &archive());
    let op = sources
        .begin_ingestion_operation("finish", JobProvider::Telegram)
        .unwrap();
    sources
        .with_ingestion_database(&op, |db| {
            let job = db.start_job("job")?;
            db.commit_job_batch(
                "job",
                &job.hash,
                &crate::corpus_database::JobBatch {
                    phase: Some(crate::corpus_database::JobPhase::Publishing),
                    checkpoint: crate::corpus_database::JobCheckpoint {
                        adapter_version: job.state.checkpoint.adapter_version,
                        cursor: None,
                        committed_batch: 1,
                        staging_generation: 1,
                    },
                    loaded_documents: 0,
                    loaded_bytes: 0,
                    indexed_documents: 0,
                    analyzed_documents: 0,
                    used_documents: 0,
                    total_documents: Some(0),
                    complete: true,
                    limitations: vec![],
                    record_ids: vec![preview_id],
                },
            )?;
            let completed = db.complete_job("job")?;
            assert!(completed.state.published_snapshot_id.is_none());
            Ok(())
        })
        .unwrap();
    let connections = sources.0.connections.clone();
    drop(op);
    drop(inputs);
    drop(sources);
    let sources = Sources::new(dir.clone(), connections);
    let op = sources
        .begin_ingestion_operation("publish", JobProvider::Telegram)
        .unwrap();
    let first = publish(&sources, &op, "job", 1002).unwrap();
    assert_eq!(first.coverage.loaded_documents, 0);
    assert!(first.document_refs.is_empty());
    assert_eq!(first.generation, 1);
    assert_eq!(publish(&sources, &op, "job", 1003).unwrap(), first);
    sources
        .with_ingestion_database(&op, |db| {
            assert_eq!(
                db.job("job")?
                    .unwrap()
                    .state
                    .published_snapshot_id
                    .as_deref(),
                Some(first.id.as_str())
            );
            Ok(())
        })
        .unwrap();
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
