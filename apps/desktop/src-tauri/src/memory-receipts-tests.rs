use super::*;

#[test]
fn recorded_receipt_expiry_permanently_closes_the_session() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let calls = Cell::new(0);
    bound
        .recall_recorded("контекст", None, || {
            calls.set(calls.get() + 1);
            70003
        })
        .unwrap();
    let final_call = calls.get();
    calls.set(0);
    assert!(bound
        .recall_recorded("контекст", None, || {
            calls.set(calls.get() + 1);
            if calls.get() >= final_call - 1 {
                130002
            } else {
                70003
            }
        })
        .is_err());
    assert!(bound
        .recall_recorded("контекст", None, || panic!(
            "expired receipt reopened session"
        ))
        .is_err());
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_receipt_records_policy_exact_sources_ranks_and_host_identity() {
    let (sources, dir) = local();
    let (op, snapshot, index) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let result = bound.recall_recorded("контекст", None, || 70003).unwrap();
    assert_eq!(result.receipt.schema_version, 2);
    assert_eq!(result.receipt.operation, "recall");
    assert_eq!(result.receipt.snapshot_id, snapshot.id);
    assert_eq!(result.receipt.index_generation, index.index_generation);
    assert_eq!(result.receipt.policy_version, "ingestionBinding:1");
    assert_eq!(result.receipt.mode, "lexical");
    assert!(result.receipt.embedding_fingerprint.is_none());
    assert_eq!(result.receipt.elapsed_ms, 0);
    assert!(!result.receipt.id.is_empty());
    assert_eq!(result.sources.len(), 1);
    assert_eq!(result.sources[0].text, "  контекст\r\n");
    assert_eq!(
        result.receipt.hits[0].spans,
        vec![result.sources[0].span.clone()]
    );
    assert_eq!(result.receipt.source_bytes, result.sources[0].text.len());
    assert_eq!(
        result.receipt.context_tokens,
        bound.usage().1.context_tokens
    );
    assert_eq!(result.receipt.hits[0].lexical_rank, Some(1));
    assert_eq!(result.receipt.hits[0].semantic_rank, None);
    let again = bound.recall_recorded("контекст", None, || 70004).unwrap();
    assert_ne!(result.receipt.id, again.receipt.id);
    let json = serde_json::to_value(&result.receipt).unwrap();
    assert_eq!(json["operation"], "recall");
    assert_eq!(json["sourceBytes"], result.sources[0].text.len());
    assert!(json.get("source_bytes").is_none());
    let empty = bound.recall_recorded("absentword", None, || 70004).unwrap();
    assert!(empty.sources.is_empty());
    assert!(empty.receipt.hits.is_empty());
    assert_eq!(empty.receipt.source_bytes, 0);
    assert_eq!(empty.receipt.context_tokens, 0);
    assert!(bound
        .recall_recorded("контекст", None, || panic!(
            "exhausted receipt touched clock"
        ))
        .is_err());
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_recorded_retrieval_rechecks_cancellation_after_receipt_construction() {
    for is_recall in [true, false] {
        let (sources, dir) = local();
        let (op, snapshot, _) = indexed(&sources, &dir, 0);
        enable_model(&sources, &op);
        let mut bound = session(&sources, &snapshot, limits()).unwrap();
        let id = episode(&sources, &snapshot.id);
        let calls = Cell::new(0);
        let clock = || {
            calls.set(calls.get() + 1);
            70003
        };
        if is_recall {
            bound.recall_recorded("контекст", None, clock).unwrap();
        } else {
            bound.read_episode_recorded(&id, clock).unwrap();
        }
        let final_call = calls.get();
        calls.set(0);
        let clock = || {
            calls.set(calls.get() + 1);
            if calls.get() == final_call {
                sources.cancel_ingestion_operation("memory").unwrap();
            }
            70003
        };
        let result = if is_recall {
            bound.recall_recorded("контекст", None, clock)
        } else {
            bound.read_episode_recorded(&id, clock)
        };
        assert!(result.is_err());
        assert_eq!(bound.usage().0, 2);
        drop(bound);
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn native_episode_receipt_has_no_invented_ranking_and_rechecks_revocation() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let id = episode(&sources, &snapshot.id);
    let result = bound.read_episode_recorded(&id, || 70003).unwrap();
    assert_eq!(result.receipt.operation, "readEpisode");
    assert_eq!(result.receipt.hits.len(), result.sources.len());
    assert!(result.receipt.hits.iter().all(|hit| hit.episode_id == id
        && hit.lexical_rank.is_none()
        && hit.semantic_rank.is_none()
        && hit.fusion_score == 0.0));
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    assert!(bound.read_episode_recorded(&id, || 70004).is_err());
    assert_eq!(bound.usage().0, 1);
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
