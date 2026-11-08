use super::super::hybrid_tests::QueryModel;
use super::*;

#[test]
fn lexical_recall_and_episode_reads_share_one_host_budget() {
    let (sources, dir) = local();
    let (op, snapshot, index) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    let recalled = bound.recall("ok", None, || 70003).unwrap();
    assert_eq!(recalled.source, index);
    assert!(recalled.vectors.is_none());
    assert!(!recalled.hits.is_empty());
    for hit in &recalled.hits {
        assert_eq!(hit.excerpt.text, hit.entry.chunk.text);
        assert_eq!(hit.excerpt.span, hit.entry.chunk.spans[0]);
        assert_eq!(
            hit.excerpt.document.id,
            hit.entry.original.reference.document_id
        );
    }
    assert!(recalled
        .hits
        .iter()
        .all(|h| h.lexical_rank.is_some() && h.semantic_rank.is_none()));
    assert_eq!(
        recalled.source_bytes,
        recalled
            .hits
            .iter()
            .map(|h| h.entry.chunk.text.len())
            .sum::<usize>()
    );
    assert_eq!(
        recalled.context_tokens,
        recalled
            .hits
            .iter()
            .map(|h| h.entry.chunk.token_count)
            .sum::<usize>()
    );
    let id = recalled.hits[0].entry.episode.id.clone();
    let expanded = bound.read_episode(&id, || 70003).unwrap();
    assert_eq!(bound.usage().0, 2);
    assert_eq!(
        bound.usage().1.source_bytes,
        recalled.source_bytes + expanded.source_bytes
    );
    assert!(bound
        .recall("word_absent", None, || 70003)
        .unwrap()
        .hits
        .is_empty());
    assert!(bound
        .read_episode(&id, || panic!("shared budget was bypassed"))
        .is_err());
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

fn hybrid_session(
    sources: &Sources,
    snapshot: &crate::ingestion::publication::SnapshotView,
    model: &dyn EmbeddingModel,
) -> Result<MemorySession, String> {
    let operation = sources
        .begin_ingestion_operation("memory", crate::corpus_database::JobProvider::Telegram)?;
    MemorySession::bind_hybrid(
        sources.clone(),
        operation,
        SessionInput {
            job_id: "job",
            snapshot_id: &snapshot.id,
            persona_id: "personA",
            period: input(snapshot).period,
            limits: limits(),
        },
        model,
        request(snapshot).budget,
        || 70002,
    )
}

#[test]
fn hybrid_session_preserves_independent_semantic_hits_and_exact_originals() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let model = QueryModel::new(&|| Ok(()));
    assert!(hybrid_session(&sources, &snapshot, &model).is_err());
    assert_eq!(model.calls.get(), 0);
    let vectors = build(
        &sources,
        &op,
        &request(&snapshot),
        &Model::new(&|_| Ok(())),
        || 70002,
    )
    .unwrap();
    let mut bound = hybrid_session(&sources, &snapshot, &model).unwrap();
    let result = bound.recall("word_absent", Some(&model), || 70003).unwrap();
    assert_eq!(result.vectors.as_ref(), Some(&vectors));
    assert_eq!(result.hits.len(), 3);
    assert!(result
        .hits
        .iter()
        .all(|h| h.lexical_rank.is_none() && h.semantic_rank.is_some()));
    assert!(result
        .hits
        .iter()
        .any(|h| h.entry.chunk.text == "  контекст\r\n"));
    assert_eq!(model.calls.get(), 1);
    assert_eq!(bound.usage().1.candidates, 3);
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn retrieval_mode_is_frozen_and_has_no_implicit_fallback() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let model = QueryModel::new(&|| panic!("wrong mode invoked inference"));
    let mut bound = session(&sources, &snapshot, limits()).unwrap();
    assert!(bound.recall("ok", Some(&model), || 70003).is_err());
    assert_eq!(bound.usage().0, 1);
    drop(bound);
    build(
        &sources,
        &op,
        &request(&snapshot),
        &Model::new(&|_| Ok(())),
        || 70002,
    )
    .unwrap();
    let mut bound = hybrid_session(&sources, &snapshot, &model).unwrap();
    assert!(bound.recall("ok", None, || 70003).is_err());
    assert_eq!(bound.usage().1.source_bytes, 0);
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recall_rechecks_model_permission_after_inference_and_closes_the_session() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    build(
        &sources,
        &op,
        &request(&snapshot),
        &Model::new(&|_| Ok(())),
        || 70002,
    )
    .unwrap();
    let revoke = || {
        sources
            .with_ingestion_database(&op, |db| db.remove(&grant))
            .unwrap();
        Ok(())
    };
    let model = QueryModel::new(&revoke);
    let mut bound = hybrid_session(&sources, &snapshot, &model).unwrap();
    assert!(bound.recall("ok", Some(&model), || 70003).is_err());
    assert_eq!(bound.usage().0, 1);
    assert_eq!(bound.usage().1.source_bytes, 0);
    assert!(bound
        .recall("ok", Some(&model), || panic!("revoked session reopened"))
        .is_err());
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn replacement_vector_generation_closes_the_bound_session_before_inference() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    build(
        &sources,
        &op,
        &request(&snapshot),
        &Model::new(&|_| Ok(())),
        || 70002,
    )
    .unwrap();
    let model = QueryModel::new(&|| panic!("changed vector pin reached inference"));
    let mut bound = hybrid_session(&sources, &snapshot, &model).unwrap();
    let mut replacement = Model::new(&|_| Ok(()));
    replacement.fingerprint = "syntheticEmbedding:2";
    build(&sources, &op, &request(&snapshot), &replacement, || 70003).unwrap();
    assert!(bound.recall("ok", Some(&model), || 70004).is_err());
    assert_eq!(bound.usage().0, 0);
    assert!(bound
        .read_episode("anything", || panic!("stale vectors reopened session"))
        .is_err());
    drop(bound);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recall_caps_disclosures_by_remaining_candidates_episodes_bytes_and_tokens() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    for resource in 0..4 {
        let mut cap = limits();
        match resource {
            0 => cap.max_candidates = 1,
            1 => cap.max_episodes = 1,
            2 => cap.max_source_bytes = 1,
            _ => cap.max_context_tokens = 1,
        }
        let mut bound = session(&sources, &snapshot, cap).unwrap();
        let result = bound.recall("ok", None, || 70003);
        if resource < 2 {
            assert_eq!(result.unwrap().hits.len(), 1);
            assert!(bound
                .recall("ok", None, || panic!(
                    "exhausted disclosure budget ran work"
                ))
                .is_err());
        } else {
            assert!(result.is_err());
            assert_eq!(bound.usage().1.source_bytes, 0);
        }
        assert_eq!(bound.usage().0, 1);
    }
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
