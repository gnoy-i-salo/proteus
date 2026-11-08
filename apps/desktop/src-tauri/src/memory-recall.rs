//! Session-bound candidate recall. Native orchestration chooses the mode and
//! runtime; model actions supply only query text. Results retain exact sources.
use super::*;
use crate::{
    corpus_database::{preparation::PreparedEntry, vectors::VectorReceipt},
    ingestion::{
        hybrid_query::{self, HybridHit, HybridQueryInput},
        search_query::{self, QueryInput},
    },
    local_embedding::{
        model::{model_error, EmbeddingModel},
        runtime::EmbeddingBudget,
    },
    retrieval_ranking::{fuse, FusionBudget, RankedReference},
};
use std::collections::HashSet;
pub struct NativeRecallHit {
    pub entry: PreparedEntry,
    pub excerpt: crate::ingestion::source_excerpt::SourceExcerpt,
    pub lexical_rank: Option<usize>,
    pub semantic_rank: Option<usize>,
    pub fusion_score: f64,
}
pub struct NativeRecallResult {
    pub source: SearchIndexReceipt,
    pub vectors: Option<VectorReceipt>,
    pub hits: Vec<NativeRecallHit>,
    pub source_bytes: usize,
    pub context_tokens: usize,
}
impl MemorySession {
    pub fn bind_hybrid(
        sources: Sources,
        operation: Operation,
        input: SessionInput<'_>,
        model: &dyn EmbeddingModel,
        budget: EmbeddingBudget,
        now_ms: impl Fn() -> i64,
    ) -> Result<Self, String> {
        budget.validate().map_err(model_error)?;
        let mut session = Self::bind(sources, operation, input, &now_ms)?;
        let scope = &mut session.scope;
        let vectors = scope
            .sources
            .with_ingestion_database(&scope.operation, |db| {
                let check = || {
                    scope.operation.check()?;
                    Ok(())
                };
                let pin = scope
                    .authority
                    .pin_index(db, &scope.source.snapshot_id, &check)?;
                let vectors = db
                    .active_vector_receipt(&scope.source.snapshot_id, &check)?
                    .ok_or("Векторный индекс не опубликован.")?;
                if pin.receipt != scope.source
                    || vectors.source != scope.source
                    || pin.tokenizer_version != model.tokenizer_fingerprint()
                    || vectors.model_fingerprint != model.fingerprint()
                    || vectors.dimensions != model.dimensions()
                {
                    return Err("Векторный индекс принадлежит другой модели или источнику.".into());
                }
                Ok(vectors)
            })?;
        scope.hybrid = Some((vectors, budget));
        scope.guard(&now_ms)?;
        Ok(session)
    }
    pub fn recall(
        &mut self,
        query_text: &str,
        model: Option<&dyn EmbeddingModel>,
        now_ms: impl Fn() -> i64,
    ) -> Result<NativeRecallResult, String> {
        let scope = &self.scope;
        self.ledger.run_checked(
            || scope.guard(&now_ms),
            |allowance| {
                let result = recall(scope, query_text, model, allowance, &now_ms)?;
                let charge = RetrievalCharge {
                    candidates: result.hits.len(),
                    episodes: result
                        .hits
                        .iter()
                        .map(|h| &h.entry.episode.id)
                        .collect::<HashSet<_>>()
                        .len(),
                    source_bytes: result.source_bytes,
                    context_tokens: result.context_tokens,
                };
                Ok((result, charge))
            },
        )
    }
}
fn recall(
    scope: &MemoryScope,
    text: &str,
    model: Option<&dyn EmbeddingModel>,
    allowance: super::super::budget::RecallAllowance,
    now_ms: &dyn Fn() -> i64,
) -> Result<NativeRecallResult, String> {
    let started = now_ms();
    let end = started
        .checked_add(allowance.duration_ms.min(60000))
        .ok_or("Неверное время поиска.")?
        .min(scope.deadline);
    let check = || {
        let now = now_ms();
        scope.operation.check()?;
        if now < started || now >= end {
            return Err("Превышено время поиска сессии.".into());
        }
        Ok(())
    };
    check()?;
    let candidates = allowance.remaining.candidates.min(10);
    let limit = candidates.min(allowance.remaining.episodes);
    let input = QueryInput {
        job_id: &scope.authority.job.id,
        snapshot_id: &scope.source.snapshot_id,
        persona_id: &scope.persona_id,
        query_text: text,
        period: scope.period.clone(),
        limit: limit as i64,
        offset: 0,
        expected_index_generation: Some(scope.source.index_generation),
        max_duration_ms: allowance.duration_ms.min(60000),
    };
    let hits = match (&scope.hybrid, model) {
        (None, None) => lexical(
            scope,
            &input,
            candidates,
            allowance.remaining.source_bytes,
            now_ms,
            &check,
        )?,
        (Some((expected, budget)), Some(model)) => {
            if expected.model_fingerprint != model.fingerprint()
                || expected.dimensions != model.dimensions()
            {
                return Err("Модель сессии изменилась.".into());
            }
            let result = hybrid_query::query(
                &scope.sources,
                &scope.operation,
                &HybridQueryInput {
                    query: input,
                    candidate_limit: candidates,
                    max_scanned: 100000,
                    max_per_document: 2,
                    max_source_bytes: allowance.remaining.source_bytes,
                    embedding_budget: EmbeddingBudget {
                        max_duration_ms: budget.max_duration_ms.min(allowance.duration_ms as u64),
                        ..*budget
                    },
                },
                model,
                now_ms,
            )?;
            if result.source != scope.source || &result.vectors != expected {
                return Err("Индекс сессии изменился.".into());
            }
            result.hits
        }
        _ => return Err("Режим поиска сессии нельзя менять во время обращения.".into()),
    };
    scope
        .sources
        .with_ingestion_database(&scope.operation, |db| {
            // Both candidate paths already enforce the same reference scope. The
            // prepared cursor now supplies authoritative episode IDs and token counts.
            let cursor = db.prepared_cursor(&scope.source.snapshot_id, &check)?;
            let ids: Vec<_> = hits.iter().map(|h| h.chunk.chunk_id.clone()).collect();
            let entries = db.prepared_entries(&cursor, &ids, 16 * 1024 * 1024, &check)?;
            if hits.len() != entries.len() {
                return Err("Неполный результат чтения.".into());
            }
            let mut result = NativeRecallResult {
                source: scope.source.clone(),
                vectors: scope.hybrid.as_ref().map(|(v, _)| v.clone()),
                hits: Vec::with_capacity(hits.len()),
                source_bytes: 0,
                context_tokens: 0,
            };
            for (hit, entry) in hits.into_iter().zip(entries) {
                let excerpt = scope.authority.verify_reference_entry(
                    &hit.chunk,
                    &entry,
                    &scope.persona_id,
                    &check,
                )?;
                result.source_bytes += entry.chunk.text.len();
                result.context_tokens += entry.chunk.token_count;
                if result.source_bytes > allowance.remaining.source_bytes
                    || result.context_tokens > allowance.remaining.context_tokens
                {
                    return Err("Превышен бюджет контекста сессии.".into());
                }
                result.hits.push(NativeRecallHit {
                    entry,
                    excerpt,
                    lexical_rank: hit.lexical_rank,
                    semantic_rank: hit.semantic_rank,
                    fusion_score: hit.fusion_score,
                });
            }
            check()?;
            Ok(result)
        })
}
fn lexical(
    scope: &MemoryScope,
    input: &QueryInput<'_>,
    candidates: usize,
    max_bytes: usize,
    now_ms: &dyn Fn() -> i64,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Vec<HybridHit>, String> {
    let response = search_query::query(
        &scope.sources,
        &scope.operation,
        &QueryInput {
            limit: candidates as i64,
            period: input.period.clone(),
            ..*input
        },
        now_ms,
    )?;
    let ranks: Vec<_> = response
        .hits
        .into_iter()
        .map(|h| RankedReference {
            chunk_id: h.chunk_id,
            document_id: h.document_id,
        })
        .collect();
    scope
        .sources
        .with_ingestion_database(&scope.operation, |db| {
            let ranked = fuse(
                &ranks,
                &[],
                &FusionBudget {
                    max_candidates_per_list: candidates,
                    max_results: input.limit as usize,
                    max_per_document: 2,
                },
                &check,
            )?;
            let pin = scope
                .authority
                .pin_index(db, &scope.source.snapshot_id, &check)?;
            if pin.receipt != scope.source {
                return Err("Индекс сессии изменился.".into());
            }
            let allowed = scope
                .authority
                .scope(&pin, &scope.persona_id, scope.period.clone())?;
            let ids: Vec<_> = ranked.iter().map(|h| h.chunk_id.clone()).collect();
            let chunks =
                db.read_reference_chunks(&allowed, &scope.source, &ids, max_bytes, &check)?;
            if chunks.len() != ranked.len() {
                return Err("Неполный результат чтения.".into());
            }
            chunks
                .into_iter()
                .zip(ranked)
                .map(|(chunk, rank)| {
                    if chunk.chunk_id != rank.chunk_id || chunk.document_id != rank.document_id {
                        return Err("Ранг не соответствует исходному фрагменту.".into());
                    }
                    Ok(HybridHit {
                        chunk,
                        lexical_rank: rank.lexical_rank,
                        semantic_rank: rank.semantic_rank,
                        fusion_score: rank.rrf_score,
                    })
                })
                .collect()
        })
}
