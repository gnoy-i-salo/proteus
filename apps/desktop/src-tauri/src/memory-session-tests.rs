use super::*;
#[path = "cli-memory-dispatch-tests.rs"]
mod cli_memory_dispatch_tests;
#[path = "memory-model-session-tests.rs"]
mod model_session_tests;
#[path = "memory-recall-tests.rs"]
mod recall_tests;
#[path = "memory-receipts-tests.rs"]
mod receipt_tests;
#[path = "memory-session-registry-tests.rs"]
mod registry_tests;
use crate::memory::{
    budget::RecallLimits,
    session::{MemorySession, SessionInput},
};
fn limits() -> RecallLimits {
    RecallLimits {
        max_calls: 3,
        max_candidates: 100,
        max_episodes: 10,
        max_source_bytes: 1024 * 1024,
        max_context_tokens: 10000,
        max_duration_ms: 60000,
    }
}
fn session(
    sources: &Sources,
    snapshot: &crate::ingestion::publication::SnapshotView,
    limits: RecallLimits,
) -> Result<MemorySession, String> {
    let operation = sources
        .begin_ingestion_operation("memory", crate::corpus_database::JobProvider::Telegram)?;
    MemorySession::bind(
        sources.clone(),
        operation,
        SessionInput {
            job_id: "job",
            snapshot_id: &snapshot.id,
            persona_id: "personA",
            period: input(snapshot).period,
            limits,
        },
        || 70002,
    )
}
fn episode(sources: &Sources, snapshot: &str) -> String {
    sources
        .with_ingestion_job_state(|db| Ok(db.prepared_snapshot(snapshot)?.episode_ids[0].clone()))
        .unwrap()
}
#[test]
fn bound_session_uses_only_remaining_budget_across_episode_reads() {
    let (sources, dir) = local();
    let (op, snapshot, index) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let id = episode(&sources, &snapshot.id);
    let mut session = session(&sources, &snapshot, limits()).unwrap();
    for n in 1..=3 {
        let result = session.read_episode(&id, || 70003).unwrap();
        assert_eq!(result.source, index);
        assert_eq!(result.episode.id, id);
        assert!(!result.entries.is_empty());
        assert_eq!(session.usage().0, n);
        assert_eq!(session.usage().1.source_bytes, n * result.source_bytes);
        assert_eq!(session.usage().1.episodes, n);
    }
    assert!(session
        .read_episode(&id, || panic!("exhausted session touched storage clock"))
        .is_err());
    drop(session);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn a_single_disclosure_can_exhaust_the_whole_session_byte_budget() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let id = episode(&sources, &snapshot.id);
    let mut probe = session(&sources, &snapshot, limits()).unwrap();
    let bytes = probe.read_episode(&id, || 70003).unwrap().source_bytes;
    assert!(bytes > 0);
    drop(probe);
    let mut limits = limits();
    limits.max_source_bytes = bytes;
    let mut bound = session(&sources, &snapshot, limits).unwrap();
    bound.read_episode(&id, || 70003).unwrap();
    assert!(bound
        .read_episode(&id, || panic!("byte exhaustion ran work"))
        .is_err());
    assert_eq!(bound.usage().0, 1);
    assert_eq!(bound.usage().1.source_bytes, bytes);
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn sessions_cannot_rebind_themselves_after_revocation_or_index_rebuild() {
    for revoke in [true, false] {
        let (sources, dir) = local();
        let (op, snapshot, _) = indexed(&sources, &dir, 0);
        let grant = enable_model(&sources, &op);
        let id = episode(&sources, &snapshot.id);
        let mut bound = session(&sources, &snapshot, limits()).unwrap();
        if revoke {
            sources
                .with_ingestion_database(&op, |db| db.remove(&grant))
                .unwrap();
        } else {
            rebuild(
                &sources,
                &op,
                &IndexInput {
                    job_id: "job",
                    snapshot_id: &snapshot.id,
                    max_duration_ms: 60000,
                },
                || 70003,
            )
            .unwrap();
        }
        assert!(bound.read_episode(&id, || 70004).is_err());
        assert_eq!(bound.usage().0, 0);
        assert!(bound
            .read_episode(&id, || panic!("closed session tried to rebind"))
            .is_err());
        drop(bound);
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn final_session_guard_cancellation_discards_the_completed_episode() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let id = episode(&sources, &snapshot.id);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let calls = Cell::new(0);
    bound
        .read_episode(&id, || {
            calls.set(calls.get() + 1);
            70003
        })
        .unwrap();
    let final_call = calls.get();
    let before = bound.usage();
    calls.set(0);
    assert!(bound
        .read_episode(&id, || {
            calls.set(calls.get() + 1);
            if calls.get() == final_call {
                sources.cancel_ingestion_operation("memory").unwrap();
            }
            70003
        })
        .is_err());
    assert_eq!(bound.usage().0, before.0 + 1);
    assert_eq!(bound.usage().1, before.1);
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn binding_requires_model_permission_and_failed_ids_still_consume_calls() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    assert!(session(&sources, &snapshot, limits()).is_err());
    enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    for _ in 0..3 {
        assert!(bound.read_episode("arbitrary-id", || 70003).is_err());
    }
    assert_eq!(bound.usage().0, 3);
    assert_eq!(bound.usage().1.source_bytes, 0);
    assert!(bound
        .read_episode(&episode(&sources, &snapshot.id), || panic!(
            "failed attempts restored calls"
        ))
        .is_err());
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn binding_cannot_return_a_session_after_its_own_deadline() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let operation = sources
        .begin_ingestion_operation("memory", crate::corpus_database::JobProvider::Telegram)
        .unwrap();
    let clock = Cell::new(70002);
    assert!(MemorySession::bind(
        sources.clone(),
        operation,
        SessionInput {
            job_id: "job",
            snapshot_id: &snapshot.id,
            persona_id: "personA",
            period: input(&snapshot).period,
            limits: limits(),
        },
        || {
            let now = clock.get();
            clock.set(130002);
            now
        }
    )
    .is_err());
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
