use super::*;
use crate::ingestion::hybrid_query::{query as hybrid_query, HybridQueryInput};

struct QueryModel<'a> {
    calls: Cell<usize>,
    on_call: &'a dyn Fn() -> Result<(), EmbeddingError>,
}
impl<'a> QueryModel<'a> {
    fn new(on_call: &'a dyn Fn() -> Result<(), EmbeddingError>) -> Self {
        Self {
            calls: Cell::new(0),
            on_call,
        }
    }
}
impl EmbeddingModel for QueryModel<'_> {
    fn fingerprint(&self) -> &str {
        "syntheticEmbedding:1"
    }
    fn tokenizer_fingerprint(&self) -> &str {
        "syntheticSearch:1"
    }
    fn dimensions(&self) -> usize {
        2
    }
    fn embed(
        &self,
        texts: &[&str],
        kind: InputKind,
        _: &EmbeddingBudget,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        assert!(matches!(kind, InputKind::Query));
        assert_eq!(texts.len(), 1);
        check().map_err(EmbeddingError::Interrupted)?;
        self.calls.set(self.calls.get() + 1);
        (self.on_call)()?;
        Ok(vec![vec![1., 0.]])
    }
}
fn hybrid_input(snapshot: &crate::ingestion::publication::SnapshotView) -> HybridQueryInput<'_> {
    HybridQueryInput {
        query: crate::ingestion::search_query::QueryInput {
            query_text: "word_absent",
            limit: 10,
            ..input(snapshot)
        },
        candidate_limit: 10,
        max_scanned: 100,
        max_per_document: 10,
        max_source_bytes: 65536,
        embedding_budget: request(snapshot).budget,
    }
}

#[test]
fn independent_semantic_candidates_return_exact_text_even_without_lexical_hits() {
    let (sources, dir) = local();
    let (op, snapshot, index) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let vectors = build(
        &sources,
        &op,
        &request(&snapshot),
        &Model::new(&|_| Ok(())),
        || 70002,
    )
    .unwrap();
    let model = QueryModel::new(&|| Ok(()));
    let mut input = hybrid_input(&snapshot);
    let result = hybrid_query(&sources, &op, &input, &model, || 70003).unwrap();
    assert_eq!(result.source, index);
    assert_eq!(result.vectors, vectors);
    assert_eq!(result.hits.len(), 3);
    assert!(result
        .hits
        .iter()
        .all(|h| h.lexical_rank.is_none() && h.semantic_rank.is_some()));
    assert!(result.hits.iter().any(|h| h.chunk.text == "  контекст\r\n"));
    assert_eq!(
        result.source_bytes,
        result
            .hits
            .iter()
            .map(|h| h.chunk.text.len())
            .sum::<usize>()
    );
    input.query.query_text = "ok";
    let mixed = hybrid_query(&sources, &op, &input, &model, || 70003).unwrap();
    assert!(mixed.hits.iter().any(|h| h.lexical_rank.is_some()));
    assert!(mixed
        .hits
        .iter()
        .any(|h| h.lexical_rank.is_none() && h.semantic_rank.is_some()));
    assert_eq!(model.calls.get(), 2);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_vectors_and_model_permission_fail_before_query_inference() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let model = QueryModel::new(&|| Ok(()));
    assert!(hybrid_query(&sources, &op, &hybrid_input(&snapshot), &model, || 70003).is_err());
    assert!(!query(&sources, &op, &input(&snapshot), || 70003)
        .unwrap()
        .hits
        .is_empty());
    enable_model(&sources, &op);
    assert!(hybrid_query(&sources, &op, &hybrid_input(&snapshot), &model, || 70003).is_err());
    assert_eq!(model.calls.get(), 0);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn holdout_and_time_scope_cannot_reenter_through_semantic_candidates() {
    for holdout in [0, 10000] {
        let (sources, dir) = local();
        let (op, snapshot, _) = indexed(&sources, &dir, holdout);
        enable_model(&sources, &op);
        build(
            &sources,
            &op,
            &request(&snapshot),
            &Model::new(&|_| Ok(())),
            || 70002,
        )
        .unwrap();
        let model = QueryModel::new(&|| Ok(()));
        let mut input = hybrid_input(&snapshot);
        if holdout == 0 {
            input.query.period.from = Some(1704067200001);
        }
        let result = hybrid_query(&sources, &op, &input, &model, || 70003).unwrap();
        assert!(result.hits.is_empty());
        assert_eq!(result.source_bytes, 0);
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn stale_generation_and_foreign_persona_fail_before_model() {
    let (sources, dir) = local();
    let (op, snapshot, index) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    build(
        &sources,
        &op,
        &request(&snapshot),
        &Model::new(&|_| Ok(())),
        || 70002,
    )
    .unwrap();
    let model = QueryModel::new(&|| panic!("stale or foreign query reached model"));
    let mut input = hybrid_input(&snapshot);
    input.query.expected_index_generation = Some(index.index_generation + 1);
    assert!(hybrid_query(&sources, &op, &input, &model, || 70003).is_err());
    input.query.expected_index_generation = None;
    input.query.persona_id = "otherPersona";
    assert!(hybrid_query(&sources, &op, &input, &model, || 70003).is_err());
    assert_eq!(model.calls.get(), 0);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn revocation_or_rebuild_during_query_inference_discards_all_results() {
    for revoke in [true, false] {
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
        let change = || {
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
            Ok(())
        };
        let model = QueryModel::new(&change);
        assert!(hybrid_query(&sources, &op, &hybrid_input(&snapshot), &model, || 70003).is_err());
        assert_eq!(
            model.calls.get(),
            1,
            "inference must not hold the database gate"
        );
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn text_and_scan_budgets_discard_partial_hybrid_results() {
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
    let model = QueryModel::new(&|| Ok(()));
    for scan in [true, false] {
        let mut input = hybrid_input(&snapshot);
        if scan {
            input.max_scanned = 1;
        } else {
            input.max_source_bytes = 1;
        }
        assert!(hybrid_query(&sources, &op, &input, &model, || 70003).is_err());
    }
    assert_eq!(model.calls.get(), 2);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_hybrid_limits_fail_before_clock_or_model() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let model = QueryModel::new(&|| panic!("invalid input reached model"));
    for field in 0..6 {
        let mut input = hybrid_input(&snapshot);
        match field {
            0 => input.candidate_limit = 0,
            1 => input.max_scanned = 100001,
            2 => input.max_per_document = 0,
            3 => input.max_source_bytes = 0,
            4 => input.query.offset = 1,
            _ => input.query.limit = 11,
        }
        assert!(hybrid_query(&sources, &op, &input, &model, || panic!(
            "invalid input reached clock"
        ))
        .is_err());
    }
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn final_clock_cancellation_never_returns_hybrid_hits() {
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
    let model = QueryModel::new(&|| Ok(()));
    let calls = Cell::new(0);
    let input = hybrid_input(&snapshot);
    hybrid_query(&sources, &op, &input, &model, || {
        calls.set(calls.get() + 1);
        70003
    })
    .unwrap();
    let final_call = calls.get();
    calls.set(0);
    let result = hybrid_query(&sources, &op, &input, &model, || {
        calls.set(calls.get() + 1);
        if calls.get() == final_call {
            sources.cancel_ingestion_operation("job").unwrap();
        }
        70003
    });
    assert!(result.is_err(), "cancelled query returned hits");
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
