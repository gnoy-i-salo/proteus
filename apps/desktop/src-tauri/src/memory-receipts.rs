//! Native audit receipts from already authorized, exact original excerpts.
use super::MemorySession;
use crate::{
    corpus_database::episodes::ChunkSpan, ingestion::source_excerpt::SourceExcerpt,
    local_embedding::model::EmbeddingModel, memory::types::EmbeddingFingerprint,
};
use serde::{Deserialize, Serialize};
use std::{cell::Cell, sync::Arc};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetrievalReceipt {
    pub schema_version: u8,
    pub operation: String,
    pub id: String,
    pub snapshot_id: String,
    pub policy_version: String,
    pub retrieval_version: String,
    pub index_generation: i64,
    #[serde(deserialize_with = "crate::memory::types::required_nullable")]
    pub embedding_fingerprint: Option<EmbeddingFingerprint>,
    pub mode: String,
    pub hits: Vec<ReceiptHit>,
    pub elapsed_ms: i64,
    pub source_bytes: usize,
    pub context_tokens: usize,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReceiptHit {
    pub chunk_id: String,
    pub episode_id: String,
    pub spans: Vec<ChunkSpan>,
    #[serde(deserialize_with = "crate::memory::types::required_nullable")]
    pub lexical_rank: Option<usize>,
    #[serde(deserialize_with = "crate::memory::types::required_nullable")]
    pub semantic_rank: Option<usize>,
    pub fusion_score: f64,
}
/// Internal host result; sources are exact excerpts, never fabricated MemoryItems.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedRetrieval {
    pub receipt: RetrievalReceipt,
    pub sources: Vec<SourceExcerpt>,
}
impl MemorySession {
    pub fn prepare_research_input(
        &mut self,
        ids: &[String],
        system_prompt: &str,
        task: &str,
        now: impl Fn() -> i64,
    ) -> Result<super::PreparedModelInput, String> {
        let receipts = self.authorized_receipts(ids, &now)?;
        let input = crate::memory::context_input::prepare(system_prompt, task, &receipts, &[])?;
        self.verify_current(now)?;
        Ok(input)
    }

    pub(crate) fn authorized_receipts(
        &mut self,
        ids: &[String],
        now: impl Fn() -> i64,
    ) -> Result<Vec<Arc<RecordedRetrieval>>, String> {
        let selected = self.history.select(ids)?;
        self.verify_current(now)?;
        Ok(selected)
    }
    fn retain_recorded(
        &mut self,
        record: RecordedRetrieval,
        now: impl Fn() -> i64,
    ) -> Result<Arc<RecordedRetrieval>, String> {
        let record = self.history.insert(record)?;
        self.verify_current(now)?;
        Ok(record)
    }
    pub fn recall_recorded(
        &mut self,
        query: &str,
        model: Option<&dyn EmbeddingModel>,
        now_ms: impl Fn() -> i64,
    ) -> Result<Arc<RecordedRetrieval>, String> {
        let started = Cell::new(None);
        let clock = || {
            let now = now_ms();
            if started.get().is_none() {
                started.set(Some(now));
            }
            now
        };
        let result = self.recall(query, model, clock)?;
        let started = started.get().ok_or("Отсутствует время поиска.")?;
        let mut receipt = self.receipt("recall", started, &now_ms)?;
        receipt.source_bytes = result.source_bytes;
        receipt.context_tokens = result.context_tokens;
        let mut sources = Vec::with_capacity(result.hits.len());
        for hit in result.hits {
            receipt.hits.push(ReceiptHit {
                chunk_id: hit.entry.chunk.id,
                episode_id: hit.entry.episode.id.clone(),
                spans: hit.entry.chunk.spans,
                lexical_rank: hit.lexical_rank,
                semantic_rank: hit.semantic_rank,
                fusion_score: hit.fusion_score,
            });
            sources.push(hit.excerpt);
        }
        receipt.elapsed_ms = self
            .ledger
            .verify(|| self.scope.guard(&now_ms))?
            .checked_sub(started)
            .filter(|v| *v >= receipt.elapsed_ms)
            .ok_or("Неверное время квитанции поиска.")?;
        self.retain_recorded(RecordedRetrieval { receipt, sources }, now_ms)
    }
    pub fn read_episode_recorded(
        &mut self,
        episode_id: &str,
        now_ms: impl Fn() -> i64,
    ) -> Result<Arc<RecordedRetrieval>, String> {
        let started = Cell::new(None);
        let clock = || {
            let now = now_ms();
            if started.get().is_none() {
                started.set(Some(now));
            }
            now
        };
        let result = self.read_episode(episode_id, clock)?;
        let started = started.get().ok_or("Отсутствует время чтения эпизода.")?;
        let mut receipt = self.receipt("readEpisode", started, &now_ms)?;
        receipt.source_bytes = result.source_bytes;
        receipt.context_tokens = result.context_tokens;
        receipt.hits = result
            .entries
            .into_iter()
            .map(|entry| ReceiptHit {
                chunk_id: entry.chunk.id,
                episode_id: entry.episode.id.clone(),
                spans: entry.chunk.spans,
                lexical_rank: None,
                semantic_rank: None,
                fusion_score: 0.0,
            })
            .collect();
        receipt.elapsed_ms = self
            .ledger
            .verify(|| self.scope.guard(&now_ms))?
            .checked_sub(started)
            .filter(|v| *v >= receipt.elapsed_ms)
            .ok_or("Неверное время квитанции поиска.")?;
        self.retain_recorded(
            RecordedRetrieval {
                receipt,
                sources: result.excerpts,
            },
            now_ms,
        )
    }
    fn receipt(
        &mut self,
        operation: &'static str,
        started: i64,
        now_ms: &dyn Fn() -> i64,
    ) -> Result<RetrievalReceipt, String> {
        let scope = &self.scope;
        let policy = scope.policy_version()?;
        if started < 0 {
            return Err("Несовместимые политики квитанции поиска.".into());
        }
        let embedding_fingerprint = scope
            .hybrid
            .as_ref()
            .map(|(vectors, _)| {
                scope
                    .sources
                    .with_ingestion_database(&scope.operation, |db| {
                        let prepared = db
                            .prepared_snapshot_checked(&scope.source.snapshot_id, &|| {
                                scope.operation.check()
                            })?;
                        crate::local_embedding::runtime::receipt_fingerprint(
                            &vectors.model_fingerprint,
                            &prepared.tokenizer_version,
                            &prepared.chunking_version,
                        )
                        .map_err(crate::local_embedding::model::model_error)
                    })
            })
            .transpose()?;
        let id = format!("retrieval:{}", crate::connections::storage::random_id(24)?);
        let elapsed_ms = self
            .ledger
            .verify(|| scope.guard(now_ms))?
            .checked_sub(started)
            .filter(|v| *v >= 0)
            .ok_or("Неверное время квитанции поиска.")?;
        Ok(RetrievalReceipt {
            schema_version: 2,
            operation: operation.into(),
            id,
            snapshot_id: scope.source.snapshot_id.clone(),
            policy_version: policy.into(),
            retrieval_version: "nativeMemoryRetrieval:1".into(),
            index_generation: scope.source.index_generation,
            embedding_fingerprint,
            mode: if scope.hybrid.is_some() {
                "hybrid".into()
            } else {
                "lexical".into()
            },
            hits: vec![],
            elapsed_ms,
            source_bytes: 0,
            context_tokens: 0,
        })
    }
}
