//! Native hybrid candidate acquisition. Scope comes from a completed job;
//! independent lexical/semantic lists share that scope and the pinned source.
//! Returned excerpts are exact bounded text, not a model-authored memory claim.
use super::{
    grants::GrantUse, index_authority::IndexAuthority, operations::Operation,
    search_query::QueryInput,
};
use crate::{
    corpus_database::{
        search::{ReferenceChunk, SearchIndexReceipt, SearchRequest},
        vectors::VectorReceipt,
    },
    local_embedding::{
        model::{model_error, EmbeddingModel},
        runtime::EmbeddingBudget,
        search::SearchBudget,
        tokenizer::InputKind,
    },
    retrieval_ranking::{fuse, FusionBudget, RankedReference},
    sources::Sources,
};
pub struct HybridQueryInput<'a> {
    pub query: QueryInput<'a>,
    pub candidate_limit: usize,
    pub max_scanned: usize,
    pub max_per_document: usize,
    pub max_source_bytes: usize,
    pub embedding_budget: EmbeddingBudget,
}
#[derive(Debug, PartialEq)]
pub struct HybridHit {
    pub chunk: ReferenceChunk,
    pub lexical_rank: Option<usize>,
    pub semantic_rank: Option<usize>,
    pub fusion_score: f64,
}
#[derive(Debug, PartialEq)]
pub struct HybridQueryResult {
    pub source: SearchIndexReceipt,
    pub vectors: VectorReceipt,
    pub hits: Vec<HybridHit>,
    pub source_bytes: usize,
}
pub fn query(
    sources: &Sources,
    operation: &Operation,
    input: &HybridQueryInput<'_>,
    model: &dyn EmbeddingModel,
    now_ms: impl Fn() -> i64,
) -> Result<HybridQueryResult, String> {
    input.query.validate()?;
    input.embedding_budget.validate().map_err(model_error)?;
    if input.query.offset != 0
        || !(1..=100).contains(&input.candidate_limit)
        || input.query.limit as usize > input.candidate_limit
        || !(1..=100000).contains(&input.max_scanned)
        || !(1..=100).contains(&input.max_per_document)
        || !(1..=64 * 1024 * 1024).contains(&input.max_source_bytes)
    {
        return Err("Неверный бюджет гибридного поиска.".into());
    }
    let started = now_ms();
    let end = started
        .checked_add(input.query.max_duration_ms)
        .filter(|_| started >= 0)
        .ok_or("Неверное время поиска.")?;
    let budget = || {
        let now = now_ms();
        operation.check()?;
        if now < started || now >= end {
            return Err("Превышено время поиска.".to_string());
        }
        Ok(now)
    };
    budget()?;
    let (authority, pin, vectors) = sources.with_ingestion_database(operation, |db| {
        let authority = IndexAuthority::resolve(
            db,
            operation,
            input.query.job_id,
            Some(input.query.persona_id),
            budget()?,
        )?;
        authority.current(db, budget()?, GrantUse::Model)?;
        let pin = authority.pin_index(db, input.query.snapshot_id, &|| budget().map(|_| ()))?;
        if input
            .query
            .expected_index_generation
            .is_some_and(|v| v != pin.receipt.index_generation)
            || pin.tokenizer_version != model.tokenizer_fingerprint()
        {
            return Err("Версия индекса или токенизатора изменилась.".into());
        }
        let vectors = db
            .active_vector_receipt(input.query.snapshot_id, &|| budget().map(|_| ()))?
            .ok_or("Векторный индекс не опубликован; требуется локальное индексирование.")?;
        if vectors.source != pin.receipt
            || vectors.model_fingerprint != model.fingerprint()
            || vectors.dimensions != model.dimensions()
        {
            return Err("Векторный индекс принадлежит другой модели или версии источников.".into());
        }
        Ok((authority, pin, vectors))
    })?;
    let check = || {
        if budget()? >= authority.deadline {
            return Err("Разрешение источника истекло во время поиска.".into());
        }
        Ok(())
    };
    let current = |db: &crate::corpus_database::CorpusDatabase| {
        check()?;
        authority.current(db, budget()?, GrantUse::Model)?;
        if db.active_search_receipt(input.query.snapshot_id)?.as_ref() != Some(&pin.receipt) {
            return Err("Индекс или источник изменился во время поиска.".into());
        }
        check()
    };
    let model_check = || sources.with_ingestion_database(operation, |db| current(db));
    model_check()?;
    let query_vectors = model
        .embed(
            &[input.query.query_text],
            InputKind::Query,
            &input.embedding_budget,
            &model_check,
        )
        .map_err(model_error)?;
    model_check()?;
    if query_vectors.len() != 1 {
        return Err("Модель вернула неверный вектор запроса.".into());
    }
    sources.with_ingestion_database(operation, |db| {
        current(db)?;
        let scope = authority.scope(&pin, input.query.persona_id, input.query.period.clone())?;
        let lexical = db.search_corpus_with_receipt(
            &SearchRequest {
                scope: scope.clone(),
                query_text: input.query.query_text.into(),
                limit: input.candidate_limit as i64,
                offset: 0,
            },
            &pin.receipt,
        )?;
        if lexical.hits.iter().any(|hit| {
            !authority.bindings.iter().any(|b| {
                hit.persona_id == b.persona_id
                    && hit.binding_id == b.id
                    && hit.grant_id == b.grant_id
            })
        }) {
            return Err("Результат не принадлежит разрешённой привязке.".into());
        }
        let semantic = db.search_vectors(
            &scope,
            &vectors,
            &query_vectors[0],
            model.fingerprint(),
            SearchBudget {
                max_scanned: input.max_scanned,
                max_results: input.candidate_limit,
            },
            &check,
        )?;
        current(db)?;
        let lexical: Vec<_> = lexical
            .hits
            .into_iter()
            .map(|hit| RankedReference {
                chunk_id: hit.chunk_id,
                document_id: hit.document_id,
            })
            .collect();
        let semantic: Vec<_> = semantic
            .into_iter()
            .map(|hit| RankedReference {
                chunk_id: hit.chunk_id,
                document_id: hit.document_id,
            })
            .collect();
        let ranked = fuse(
            &lexical,
            &semantic,
            &FusionBudget {
                max_candidates_per_list: input.candidate_limit,
                max_results: input.query.limit as usize,
                max_per_document: input.max_per_document,
            },
            &check,
        )?;
        let ids = ranked
            .iter()
            .map(|hit| hit.chunk_id.clone())
            .collect::<Vec<_>>();
        let chunks =
            db.read_reference_chunks(&scope, &pin.receipt, &ids, input.max_source_bytes, &check)?;
        if chunks.len() != ranked.len() {
            return Err("Неполные результаты чтения фрагментов.".into());
        }
        let mut hits = Vec::with_capacity(chunks.len());
        let mut source_bytes = 0;
        for (chunk, rank) in chunks.into_iter().zip(ranked) {
            if chunk.chunk_id != rank.chunk_id
                || chunk.document_id != rank.document_id
                || !authority.bindings.iter().any(|b| {
                    b.persona_id == chunk.persona_id
                        && b.id == chunk.binding_id
                        && b.grant_id == chunk.grant_id
                })
            {
                return Err("Результат не принадлежит разрешённой привязке.".into());
            }
            source_bytes += chunk.text.len();
            hits.push(HybridHit {
                chunk,
                lexical_rank: rank.lexical_rank,
                semantic_rank: rank.semantic_rank,
                fusion_score: rank.rrf_score,
            });
        }
        current(db)?;
        if db
            .active_vector_receipt(input.query.snapshot_id, &check)?
            .as_ref()
            != Some(&vectors)
        {
            return Err("Векторный индекс изменился во время поиска.".into());
        }
        check()?;
        Ok(HybridQueryResult {
            source: pin.receipt.clone(),
            vectors,
            hits,
            source_bytes,
        })
    })
}
