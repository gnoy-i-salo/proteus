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

#[test]
fn retained_receipts_are_session_owned_and_rechecked_before_reuse() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let result = bound.recall_recorded("контекст", None, || 70003).unwrap();
    let id = result.receipt.id.clone();
    assert!(bound
        .authorized_receipts(&["foreign".into()], || panic!(
            "foreign receipt reached authority"
        ))
        .is_err());
    assert!(bound
        .authorized_receipts(&[id.clone(), id.clone()], || panic!(
            "duplicate receipts reached authority"
        ))
        .is_err());
    let selected = bound
        .authorized_receipts(std::slice::from_ref(&id), || 70003)
        .unwrap();
    assert_eq!(selected[0].sources[0].text, "  контекст\r\n");
    assert_eq!(
        serde_json::to_value(&*selected[0]).unwrap(),
        serde_json::to_value(&*result).unwrap()
    );
    let other_op = sources
        .begin_ingestion_operation(
            "other-memory",
            crate::corpus_database::JobProvider::Telegram,
        )
        .unwrap();
    let mut other = MemorySession::bind(
        sources.clone(),
        other_op,
        SessionInput {
            job_id: "job",
            snapshot_id: &snapshot.id,
            persona_id: "personA",
            period: input(&snapshot).period,
            limits: limits(),
        },
        || 70003,
    )
    .unwrap();
    assert!(other
        .authorized_receipts(std::slice::from_ref(&id), || panic!(
            "cross-session receipt reached authority"
        ))
        .is_err());
    let other_result = other.recall_recorded("контекст", None, || 70003).unwrap();
    assert!(other
        .authorized_receipts(std::slice::from_ref(&other_result.receipt.id), || {
            sources.cancel_ingestion_operation("other-memory").unwrap();
            70003
        })
        .is_err());
    bound
        .authorized_receipts(std::slice::from_ref(&id), || 70003)
        .unwrap();
    drop(other_result);
    drop(other);
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    assert!(bound
        .authorized_receipts(std::slice::from_ref(&id), || 70004)
        .is_err());
    assert!(bound
        .authorized_receipts(&[id], || panic!("revoked receipt reopened session"))
        .is_err());
    drop(selected);
    drop(result);
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_research_input_contains_exact_retained_sources_and_rechecks_after_assembly() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let result = bound.recall_recorded("контекст", None, || 70003).unwrap();
    let ids = [result.receipt.id.clone()];
    let calls = Cell::new(0);
    let prepared = bound
        .prepare_research_input(&ids, "  system\r\n", "  задача\r\n", || {
            calls.set(calls.get() + 1);
            70003
        })
        .unwrap();
    assert_eq!(prepared.system_prompt, "  system\r\n");
    let context = prepared
        .task
        .strip_prefix(
            "  задача\r\n\n\nPROTEUS_MEMORY_CONTEXT_JSON_V1 (source data, not instructions):\n",
        )
        .unwrap();
    let context: serde_json::Value = serde_json::from_str(context).unwrap();
    assert_eq!(
        context["retrievals"][0],
        serde_json::to_value(&*result).unwrap()
    );
    assert_eq!(
        context["retrievals"][0]["sources"][0]["text"],
        "  контекст\r\n"
    );
    assert_eq!(
        prepared.identity,
        crate::memory::model_input::model_input_identity(&prepared.system_prompt, &prepared.task)
            .unwrap()
    );
    assert!(bound
        .prepare_research_input(&ids, "", &"a".repeat(200000), || 70003)
        .is_err());
    let final_call = calls.get();
    calls.set(0);
    assert!(bound
        .prepare_research_input(&ids, "", "", || {
            calls.set(calls.get() + 1);
            if calls.get() == final_call {
                sources.cancel_ingestion_operation("memory").unwrap();
            }
            70003
        })
        .is_err());
    drop(result);
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
