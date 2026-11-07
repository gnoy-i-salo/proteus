use super::ingestion_fixture::{archive, prepare};
use super::*;
use crate::corpus_database::{JobProvider, JobStatus};
use crate::ingestion::job_workflow::{self, Control, JobView, Restart, StartRequest};
fn request(sources: &Sources, op: &crate::ingestion::operations::Operation) -> StartRequest {
    sources
        .with_ingestion_database(op, |db| {
            let original = db.job("job")?.unwrap();
            Ok(StartRequest {
                operation_id: "controlled".into(),
                selection: original.state.selection,
                budget: original.state.budget,
            })
        })
        .unwrap()
}
#[test]
fn start_validates_native_selection_and_returns_a_canonical_job_view_without_private_hash() {
    let (sources, dir) = local();
    let (inputs, _, _) = prepare(&sources, &dir, &archive());
    let op = sources
        .begin_ingestion_operation("controlled", JobProvider::Telegram)
        .unwrap();
    let job = job_workflow::create(&sources, &inputs, &op, request(&sources, &op), 1001).unwrap();
    assert_eq!(job.status, JobStatus::Running);
    let view = serde_json::to_value(JobView::from(job.clone())).unwrap();
    assert_eq!(view["schemaVersion"], 1);
    assert_eq!(view["id"], "controlled");
    assert_eq!(view["coverage"]["loadedDocuments"], 0);
    assert!(view.get("hash").is_none());
    assert!(view.get("state").is_none());
    assert!(view.get("publishedSnapshotId").unwrap().is_null());
    assert_eq!(job_workflow::inspect(&sources, "controlled").unwrap(), job);
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn pause_and_immediate_resume_preserve_checkpoint_and_do_not_revive_the_old_worker() {
    let (sources, dir) = local();
    let (inputs, _, _) = prepare(&sources, &dir, &archive());
    let old = sources
        .begin_ingestion_operation("controlled", JobProvider::Telegram)
        .unwrap();
    job_workflow::create(&sources, &inputs, &old, request(&sources, &old), 1001).unwrap();
    let paused = job_workflow::control(&sources, "controlled", Control::Pause).unwrap();
    assert_eq!(paused.status, JobStatus::Paused);
    assert!(old.check().is_err());
    let (resumed, new) =
        job_workflow::restart(&sources, &inputs, "controlled", Restart::Resume, 1002).unwrap();
    assert_eq!(resumed.status, JobStatus::Running);
    assert_eq!(resumed.state.checkpoint, paused.state.checkpoint);
    assert!(new.check().is_ok());
    drop(old);
    let cancelled = job_workflow::control(&sources, "controlled", Control::Cancel).unwrap();
    assert_eq!(cancelled.status, JobStatus::Cancelled);
    assert!(new.check().is_err());
    assert!(job_workflow::restart(&sources, &inputs, "controlled", Restart::Resume, 1003).is_err());
    assert!(job_workflow::restart(&sources, &inputs, "controlled", Restart::Retry, 1003).is_err());
    drop(new);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn released_file_forged_operation_or_grant_never_create_a_job() {
    for case in 0..3 {
        let (sources, dir) = local();
        let (inputs, _, _) = prepare(&sources, &dir, &archive());
        let op = sources
            .begin_ingestion_operation("controlled", JobProvider::Telegram)
            .unwrap();
        let mut r = request(&sources, &op);
        match case {
            0 => inputs.release("selected").unwrap(),
            1 => r.operation_id = "forged".into(),
            _ => r.selection.bindings[0].allow_unknown_context = true,
        }
        assert!(job_workflow::create(&sources, &inputs, &op, r, 1001).is_err());
        sources
            .with_ingestion_database(&op, |db| {
                assert!(db.job("controlled")?.is_none());
                assert!(db.job("forged")?.is_none());
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
fn execution_persists_completion_and_unavailable_inputs_become_retryable_failures() {
    for unavailable in [false, true] {
        let (sources, dir) = local();
        let (inputs, _, _) = prepare(&sources, &dir, &archive());
        let op = sources
            .begin_ingestion_operation("controlled", JobProvider::Telegram)
            .unwrap();
        job_workflow::create(&sources, &inputs, &op, request(&sources, &op), 1001).unwrap();
        if unavailable {
            inputs.release("selected").unwrap();
        }
        let job = job_workflow::execute(&sources, &inputs, &op, || 1002).unwrap();
        assert_eq!(
            job.status,
            if unavailable {
                JobStatus::Failed
            } else {
                JobStatus::Completed
            }
        );
        if unavailable {
            assert!(job.state.error.as_ref().unwrap().retryable);
            assert!(job.state.published_snapshot_id.is_none());
        } else {
            assert!(job.state.error.is_none());
            assert_eq!(job.state.coverage.loaded_documents, 3);
            assert!(job.state.published_snapshot_id.is_some());
        }
        assert_eq!(job_workflow::inspect(&sources, "controlled").unwrap(), job);
        drop(op);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn late_failure_from_a_retired_worker_cannot_fail_or_cancel_a_resumed_job() {
    let (sources, dir) = local();
    let (inputs, _, _) = prepare(&sources, &dir, &archive());
    let old = sources
        .begin_ingestion_operation("controlled", JobProvider::Telegram)
        .unwrap();
    job_workflow::create(&sources, &inputs, &old, request(&sources, &old), 1001).unwrap();
    job_workflow::control(&sources, "controlled", Control::Pause).unwrap();
    let (resumed, new) =
        job_workflow::restart(&sources, &inputs, "controlled", Restart::Resume, 1002).unwrap();
    let actual = job_workflow::finish_failure(
        &sources,
        &old,
        crate::ingestion::telegram_worker::Failure {
            message: "old read interrupted".into(),
            retryable: true,
        },
        1003,
    )
    .unwrap();
    assert_eq!(actual, resumed);
    assert!(new.check().is_ok());
    assert!(actual.state.error.is_none());
    drop(old);
    drop(new);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn cancelling_and_inspecting_jobs_remains_available_at_full_worker_capacity() {
    let (sources, dir) = local();
    let (inputs, _, _) = prepare(&sources, &dir, &archive());
    let op = sources
        .begin_ingestion_operation("controlled", JobProvider::Telegram)
        .unwrap();
    job_workflow::create(&sources, &inputs, &op, request(&sources, &op), 1001).unwrap();
    let workers = (0..15)
        .map(|i| {
            sources
                .begin_ingestion_operation(&format!("other-{i}"), JobProvider::Telegram)
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(sources
        .begin_ingestion_operation("overflow", JobProvider::Telegram)
        .is_err());
    assert_eq!(
        job_workflow::inspect(&sources, "controlled")
            .unwrap()
            .status,
        JobStatus::Running
    );
    assert_eq!(
        job_workflow::control(&sources, "controlled", Control::Cancel)
            .unwrap()
            .status,
        JobStatus::Cancelled
    );
    assert!(op.check().is_err());
    assert!(workers.iter().all(|worker| worker.check().is_ok()));
    drop(workers);
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn application_shutdown_pauses_work_for_restart_instead_of_permanently_cancelling_it() {
    let (sources, dir) = local();
    let (inputs, _, _) = prepare(&sources, &dir, &archive());
    let op = sources
        .begin_ingestion_operation("controlled", JobProvider::Telegram)
        .unwrap();
    job_workflow::create(&sources, &inputs, &op, request(&sources, &op), 1001).unwrap();
    sources.shutdown();
    let stopped = job_workflow::execute(&sources, &inputs, &op, || 1002).unwrap();
    assert_eq!(stopped.status, JobStatus::Paused);
    assert!(stopped.state.published_snapshot_id.is_none());
    let connections = sources.0.connections.clone();
    drop(op);
    drop(inputs);
    drop(sources);
    let restored = Sources::new(dir.clone(), connections);
    assert_eq!(
        job_workflow::inspect(&restored, "controlled")
            .unwrap()
            .status,
        JobStatus::Paused
    );
    drop(restored);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn explicit_operation_cancellation_is_durable_and_cannot_publish() {
    let (sources, dir) = local();
    let (inputs, _, _) = prepare(&sources, &dir, &archive());
    let op = sources
        .begin_ingestion_operation("controlled", JobProvider::Telegram)
        .unwrap();
    job_workflow::create(&sources, &inputs, &op, request(&sources, &op), 1001).unwrap();
    sources.cancel_ingestion_operation("controlled").unwrap();
    let stopped = job_workflow::execute(&sources, &inputs, &op, || 1002).unwrap();
    assert_eq!(stopped.status, JobStatus::Cancelled);
    assert!(stopped.state.published_snapshot_id.is_none());
    drop(op);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn retry_uses_the_existing_job_and_enforces_the_persisted_retryability_decision() {
    for retryable in [true, false] {
        let (sources, dir) = local();
        let (inputs, _, _) = prepare(&sources, &dir, &archive());
        let op = sources
            .begin_ingestion_operation("controlled", JobProvider::Telegram)
            .unwrap();
        job_workflow::create(&sources, &inputs, &op, request(&sources, &op), 1001).unwrap();
        let failed = job_workflow::finish_failure(
            &sources,
            &op,
            crate::ingestion::telegram_worker::Failure {
                message: "read failure".into(),
                retryable,
            },
            1002,
        )
        .unwrap();
        assert_eq!(failed.status, JobStatus::Failed);
        drop(op);
        let restarted =
            job_workflow::restart(&sources, &inputs, "controlled", Restart::Retry, 1003);
        assert_eq!(restarted.is_ok(), retryable);
        if let Ok((running, op)) = restarted {
            assert_eq!(running.id, failed.id);
            assert_eq!(running.state.checkpoint, failed.state.checkpoint);
            assert_eq!(
                job_workflow::execute(&sources, &inputs, &op, || 1004)
                    .unwrap()
                    .status,
                JobStatus::Completed
            );
        }
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
