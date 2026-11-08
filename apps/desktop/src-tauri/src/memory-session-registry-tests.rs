use super::*;
use crate::memory::registry::MemorySessions;
use std::sync::{mpsc, Arc};
use std::time::Duration;

fn named_session(
    sources: &Sources,
    snapshot: &crate::ingestion::publication::SnapshotView,
    id: &str,
) -> MemorySession {
    let operation = sources
        .begin_ingestion_operation(id, crate::corpus_database::JobProvider::Telegram)
        .unwrap();
    MemorySession::bind(
        sources.clone(),
        operation,
        SessionInput {
            job_id: "job",
            snapshot_id: &snapshot.id,
            persona_id: "personA",
            period: input(snapshot).period,
            limits: limits(),
        },
        || 70002,
    )
    .unwrap()
}

#[test]
fn registry_handles_preserve_bound_scope_and_cannot_reopen_after_close() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let registry = MemorySessions::default();
    let id = registry
        .insert(named_session(&sources, &snapshot, "registry-one"), || 70003)
        .unwrap();
    assert_ne!(id, "registry-one");
    let result = registry
        .with_session(
            &id,
            || 70003,
            |session| session.recall_recorded("контекст", None, || 70003),
        )
        .unwrap();
    assert_eq!(result.sources[0].text, "  контекст\r\n");
    assert!(registry
        .with_session(
            "foreign",
            || panic!("foreign handle checked authority"),
            |_| Ok(())
        )
        .is_err());
    assert!(registry.close(&id).unwrap());
    assert!(!registry.close(&id).unwrap());
    assert!(registry
        .with_session(
            &id,
            || panic!("closed handle checked authority"),
            |_| Ok(())
        )
        .is_err());
    let second = registry
        .insert(named_session(&sources, &snapshot, "registry-one"), || 70003)
        .unwrap();
    assert_ne!(id, second);
    registry.shutdown();
    drop(registry);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn registry_close_cancels_busy_session_without_waiting_for_its_mutex() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let registry = Arc::new(MemorySessions::default());
    let id = registry
        .insert(named_session(&sources, &snapshot, "registry-busy"), || {
            70003
        })
        .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let worker_registry = registry.clone();
    let worker_id = id.clone();
    let worker = std::thread::spawn(move || {
        worker_registry.with_session(
            &worker_id,
            || 70003,
            |_| {
                started_tx.send(()).unwrap();
                resume_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok("must not escape after close")
            },
        )
    });
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(registry
        .with_session(&id, || panic!("busy session touched authority"), |_| Ok(()))
        .is_err());
    assert!(registry.close(&id).unwrap());
    resume_tx.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    drop(registry);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn registry_rechecks_grants_before_work_and_shutdown_denies_new_handles() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let registry = MemorySessions::default();
    let id = registry
        .insert(
            named_session(&sources, &snapshot, "registry-grants"),
            || 70003,
        )
        .unwrap();
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    assert!(registry
        .with_session::<()>(&id, || 70003, |_| panic!("revoked session reached work"))
        .is_err());
    registry.shutdown();
    assert!(registry
        .with_session(&id, || panic!("shutdown reached authority"), |_| Ok(()))
        .is_err());
    drop(registry);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();

    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let registry = MemorySessions::default();
    registry.shutdown();
    assert!(registry
        .insert(named_session(&sources, &snapshot, "after-shutdown"), || {
            70003
        })
        .is_err());
    registry.shutdown();
    drop(registry);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn registry_admission_has_a_global_limit_without_evicting_live_handles() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    drop(op);
    let registry = Arc::new(MemorySessions::default());
    let mut ids = Vec::new();
    for n in 0..16 {
        ids.push(
            registry
                .insert(
                    named_session(&sources, &snapshot, &format!("bounded-{n}")),
                    || 70003,
                )
                .unwrap(),
        );
    }
    let (other_sources, other_dir) = local();
    let (other_op, other_snapshot, _) = indexed(&other_sources, &other_dir, 0);
    enable_model(&other_sources, &other_op);
    assert!(registry
        .insert(
            named_session(&other_sources, &other_snapshot, "overflow"),
            || 70003
        )
        .is_err());
    registry
        .with_session(&ids[0], || 70003, |_| Ok(()))
        .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let worker_registry = registry.clone();
    let worker_id = ids[0].clone();
    let worker = std::thread::spawn(move || {
        worker_registry.with_session(
            &worker_id,
            || 70003,
            |_| {
                started_tx.send(()).unwrap();
                resume_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(())
            },
        )
    });
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    registry.close(&ids[0]).unwrap();
    assert!(registry
        .insert(
            named_session(&other_sources, &other_snapshot, "still-retiring"),
            || 70003
        )
        .is_err());
    resume_tx.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    registry
        .insert(
            named_session(&other_sources, &other_snapshot, "after-capacity"),
            || 70003,
        )
        .unwrap();
    registry.shutdown();
    drop(registry);
    drop(other_op);
    drop(other_sources);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(other_dir).unwrap();
}

#[test]
fn registry_rejects_results_revoked_during_work_and_keeps_original_errors() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let registry = MemorySessions::default();
    let id = registry
        .insert(named_session(&sources, &snapshot, "late-revoke"), || 70003)
        .unwrap();
    let result = registry.with_session(
        &id,
        || 70003,
        |_| {
            sources.with_ingestion_database(&op, |db| db.remove(&grant))?;
            Ok("revoked result")
        },
    );
    assert!(result.is_err());
    assert!(registry
        .with_session(&id, || panic!("closed ledger reread authority"), |_| Ok(()))
        .is_err());
    drop(registry);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();

    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let registry = MemorySessions::default();
    let id = registry
        .insert(named_session(&sources, &snapshot, "late-shutdown"), || {
            70003
        })
        .unwrap();
    let result = registry.with_session::<()>(
        &id,
        || 70003,
        |_| {
            registry.shutdown();
            Err("original error\r\n".into())
        },
    );
    assert_eq!(result.unwrap_err(), "original error\r\n");
    assert!(registry
        .with_session(&id, || panic!("shutdown reread authority"), |_| Ok(()))
        .is_err());
    drop(registry);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
