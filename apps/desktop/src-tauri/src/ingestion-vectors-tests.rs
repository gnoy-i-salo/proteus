use super::*;
use crate::{
    ingestion::vectors::{build, VectorBuildInput},
    local_embedding::{
        model::EmbeddingModel,
        runtime::{EmbeddingBudget, EmbeddingError, RESIDENT_RESERVATION_BYTES},
        tokenizer::InputKind,
    },
};
use std::cell::{Cell, RefCell};
struct Model<'a> {
    seen: RefCell<Vec<Vec<String>>>,
    on_call: &'a dyn Fn(usize) -> Result<(), EmbeddingError>,
    tokenizer: &'static str,
    fingerprint: &'static str,
    short: bool,
}
impl<'a> Model<'a> {
    fn new(on_call: &'a dyn Fn(usize) -> Result<(), EmbeddingError>) -> Self {
        Self {
            seen: RefCell::new(Vec::new()),
            on_call,
            tokenizer: "syntheticSearch:1",
            fingerprint: "syntheticEmbedding:1",
            short: false,
        }
    }
}
impl EmbeddingModel for Model<'_> {
    fn fingerprint(&self) -> &str {
        self.fingerprint
    }
    fn tokenizer_fingerprint(&self) -> &str {
        self.tokenizer
    }
    fn dimensions(&self) -> usize {
        2
    }
    fn embed(
        &self,
        texts: &[&str],
        kind: InputKind,
        budget: &EmbeddingBudget,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        assert!(matches!(kind, InputKind::Passage));
        assert!(texts.len() <= budget.max_batch_items);
        check().map_err(EmbeddingError::Interrupted)?;
        self.seen
            .borrow_mut()
            .push(texts.iter().map(|s| s.to_string()).collect());
        (self.on_call)(self.seen.borrow().len())?;
        let mut values: Vec<_> = texts.iter().map(|s| vec![s.len() as f32, 1.]).collect();
        if self.short {
            values.pop();
        }
        Ok(values)
    }
}
fn request(snapshot: &crate::ingestion::publication::SnapshotView) -> VectorBuildInput<'_> {
    VectorBuildInput {
        job_id: "job",
        snapshot_id: &snapshot.id,
        budget: EmbeddingBudget {
            max_batch_items: 1,
            max_batch_tokens: 512,
            max_resident_bytes: RESIDENT_RESERVATION_BYTES,
            max_duration_ms: 60000,
        },
    }
}
fn enable_model(sources: &Sources, op: &crate::ingestion::operations::Operation) -> String {
    sources
        .with_ingestion_database(op, |db| {
            let id = db.job("job")?.unwrap().state.selection.bindings[0]
                .grant_id
                .clone();
            let mut input = db.read(&id)?.unwrap().input;
            let mut grant: serde_json::Value = serde_json::from_str(&input.state_json).unwrap();
            grant["allowModelProcessing"] = true.into();
            input.state_json = serde_json::to_string(&grant).unwrap();
            db.save_ingestion(&input)?;
            Ok(id)
        })
        .unwrap()
}
fn assert_unpublished(sources: &Sources, snapshot: &str) {
    assert!(sources
        .with_ingestion_job_state(|db| db.active_vector_receipt(snapshot, &|| Ok(())))
        .unwrap()
        .is_none());
}
#[test]
fn authorized_model_build_streams_exact_reference_text_and_reuses_published_generation() {
    let (sources, dir) = local();
    let (op, snapshot, index) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let model = Model::new(&|_| Ok(()));
    let receipt = build(&sources, &op, &request(&snapshot), &model, || 70002).unwrap();
    assert_eq!(receipt.source, index);
    assert_eq!(receipt.vector_count, index.chunk_count);
    assert_eq!(receipt.model_fingerprint, model.fingerprint());
    assert_eq!(receipt.dimensions, 2);
    assert_eq!(model.seen.borrow().len(), index.chunk_count as usize);
    assert!(model
        .seen
        .borrow()
        .iter()
        .flatten()
        .any(|s| s == "  контекст\r\n"));
    assert!(model
        .seen
        .borrow()
        .iter()
        .flatten()
        .all(|s| !s.contains("excluded")));
    let calls = model.seen.borrow().len();
    assert_eq!(
        build(&sources, &op, &request(&snapshot), &model, || 70003).unwrap(),
        receipt
    );
    assert_eq!(
        model.seen.borrow().len(),
        calls,
        "published generation must not rerun inference"
    );
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn corpus_permission_does_not_authorize_embedding_and_wrong_tokenizer_never_runs() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let mut model = Model::new(&|_| Ok(()));
    assert!(build(&sources, &op, &request(&snapshot), &model, || 70002).is_err());
    assert!(model.seen.borrow().is_empty());
    enable_model(&sources, &op);
    model.tokenizer = "otherTokenizer";
    assert!(build(&sources, &op, &request(&snapshot), &model, || 70002).is_err());
    assert!(model.seen.borrow().is_empty());
    assert_unpublished(&sources, &snapshot.id);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn all_holdout_publishes_an_empty_index_without_model_text() {
    let (sources, dir) = local();
    let (op, snapshot, index) = indexed(&sources, &dir, 10000);
    enable_model(&sources, &op);
    let model = Model::new(&|_| panic!("holdout reached model"));
    let result = build(&sources, &op, &request(&snapshot), &model, || 70002).unwrap();
    assert_eq!(result.source, index);
    assert_eq!(result.vector_count, 0);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn revoked_permission_during_inference_cannot_publish_and_model_does_not_hold_database_gate() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let revoke = |_: usize| {
        sources
            .with_ingestion_database(&op, |db| db.remove(&grant))
            .unwrap();
        Ok(())
    };
    let model = Model::new(&revoke);
    assert!(build(&sources, &op, &request(&snapshot), &model, || 70002).is_err());
    assert_eq!(
        model.seen.borrow().len(),
        1,
        "revocation reached the model boundary"
    );
    assert_unpublished(&sources, &snapshot.id);
    drop(model);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn interrupted_partial_build_can_retry_without_losing_the_pinned_stage() {
    let (sources, dir) = local();
    let (op, snapshot, index) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let failure = |call| {
        if call == 2 {
            Err(EmbeddingError::Interrupted(
                "retry after interruption  ".into(),
            ))
        } else {
            Ok(())
        }
    };
    let first = Model::new(&failure);
    assert_eq!(
        build(&sources, &op, &request(&snapshot), &first, || 70002).unwrap_err(),
        "retry after interruption  "
    );
    assert_unpublished(&sources, &snapshot.id);
    let second = Model::new(&|_| Ok(()));
    let receipt = build(&sources, &op, &request(&snapshot), &second, || 70003).unwrap();
    assert_eq!(receipt.vector_count, index.chunk_count);
    assert_eq!(
        second.seen.borrow().len(),
        index.chunk_count as usize - 1,
        "already persisted vectors must not rerun inference on resume"
    );
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn changed_source_generation_during_inference_discards_the_vectors() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let change = |_: usize| {
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
        Ok(())
    };
    let model = Model::new(&change);
    assert!(build(&sources, &op, &request(&snapshot), &model, || 70002).is_err());
    assert_eq!(model.seen.borrow().len(), 1);
    assert_unpublished(&sources, &snapshot.id);
    drop(model);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn malformed_model_batch_and_expired_budget_never_publish_partial_results() {
    for expire in [false, true] {
        let (sources, dir) = local();
        let (op, snapshot, _) = indexed(&sources, &dir, 0);
        enable_model(&sources, &op);
        let time = Cell::new(70002);
        let advance = |_: usize| {
            if expire {
                time.set(200000);
            }
            Ok(())
        };
        let mut model = Model::new(&advance);
        model.short = !expire;
        assert!(build(&sources, &op, &request(&snapshot), &model, || time.get()).is_err());
        assert_eq!(model.seen.borrow().len(), 1);
        assert_unpublished(&sources, &snapshot.id);
        drop(model);
        drop(op);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn real_e5_vectors_connect_authorized_import_preparation_and_hybrid_search() {
    use crate::local_embedding::{
        runtime::{E5Runtime, ModelFiles},
        search::SearchBudget,
        tokenizer::E5Tokenizer,
    };
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.cache/models");
    let files = ModelFiles {
        weights: directory.join("e5-model.safetensors"),
        config: directory.join("e5-config.json"),
        tokenizer: directory.join("e5-tokenizer.json"),
    };
    let tokenizer = E5Tokenizer::load(&files.tokenizer).unwrap();
    let (sources, dir) = local();
    let (op, snapshot) = prepared_with_tokenizer(&sources, &dir, 0, &tokenizer);
    let index = rebuild(
        &sources,
        &op,
        &IndexInput {
            job_id: "job",
            snapshot_id: &snapshot.id,
            max_duration_ms: 60000,
        },
        || 70001,
    )
    .unwrap();
    enable_model(&sources, &op);
    let mut input = request(&snapshot);
    input.budget.max_batch_items = 8;
    input.budget.max_batch_tokens = 4096;
    let model = E5Runtime::load(&files, &input.budget, &|| Ok(())).unwrap();
    let receipt = build(&sources, &op, &input, &model, || 70002).unwrap();
    assert_eq!(receipt.vector_count, index.chunk_count);
    assert_eq!(receipt.dimensions, 384);
    let query = model
        .embed(
            &["контекст"],
            InputKind::Query,
            model.fingerprint(),
            &input.budget,
            &|| Ok(()),
        )
        .unwrap();
    sources
        .with_ingestion_database(&op, |db| {
            let binding = &db.job("job")?.unwrap().state.selection.bindings[0];
            let scope = SearchScope {
                snapshot_id: snapshot.id.clone(),
                snapshot_generation: snapshot.generation,
                persona_id: binding.persona_id.clone(),
                grant_ids: vec![binding.grant_id.clone()],
                binding_ids: vec![binding.id.clone()],
                period: SearchPeriod {
                    from: None,
                    to: None,
                    include_undated: true,
                },
            };
            let hits = db.search_vectors(
                &scope,
                &receipt,
                &query[0],
                model.fingerprint(),
                SearchBudget {
                    max_scanned: 100,
                    max_results: 100,
                },
                &|| Ok(()),
            )?;
            assert_eq!(hits.len(), index.chunk_count as usize);
            assert!(hits.iter().all(|h| h.cosine.is_finite()));
            assert_eq!(
                db.active_vector_receipt(&snapshot.id, &|| Ok(()))?,
                Some(receipt)
            );
            Ok(())
        })
        .unwrap();
    let hybrid = crate::ingestion::hybrid_query::query(
        &sources,
        &op,
        &crate::ingestion::hybrid_query::HybridQueryInput {
            query: super::input(&snapshot),
            candidate_limit: 10,
            max_scanned: 100,
            max_per_document: 10,
            max_source_bytes: 65536,
            embedding_budget: input.budget,
        },
        &model,
        || 70003,
    )
    .unwrap();
    assert_eq!(hybrid.hits.len(), index.chunk_count as usize);
    assert!(hybrid.hits.iter().any(|hit| {
        hit.chunk.text == "  контекст\r\n"
            && hit.semantic_rank.is_some()
            && hit.lexical_rank.is_some()
    }));
    drop(model);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn final_clock_boundary_cancellation_discards_even_a_published_replay() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let model = Model::new(&|_| Ok(()));
    build(&sources, &op, &request(&snapshot), &model, || 70002).unwrap();
    let calls = Cell::new(0);
    build(&sources, &op, &request(&snapshot), &model, || {
        calls.set(calls.get() + 1);
        70002
    })
    .unwrap();
    let final_call = calls.get();
    calls.set(0);
    let result = build(&sources, &op, &request(&snapshot), &model, || {
        calls.set(calls.get() + 1);
        if calls.get() == final_call {
            sources.cancel_ingestion_operation("job").unwrap();
        }
        70002
    });
    assert!(
        result.is_err(),
        "cancelled replay returned a success receipt"
    );
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[path = "ingestion-hybrid-query-tests.rs"]
mod hybrid_tests;

#[path = "ingestion-episode-read-tests.rs"]
mod episode_read_tests;

#[path = "memory-session-tests.rs"]
mod memory_session_tests;
