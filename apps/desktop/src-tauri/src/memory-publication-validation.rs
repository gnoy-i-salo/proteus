use super::ResearchPublication;
use crate::memory::{
    session::{CompletedModelContext, RetrievalReceipt},
    types::{
        validate_host_id, validate_limitations, validate_span, validate_timestamp,
        validate_unique_host_ids,
    },
};
use std::collections::HashSet;
const INVALID: &str = "Журнал публикации памяти повреждён или не соответствует исследованию.";
const SAFE_INTEGER: u64 = 9_007_199_254_740_991;
fn id(value: &str) -> Result<(), String> {
    validate_host_id(value, "журнал исследования")
}
fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn context(value: &CompletedModelContext) -> Result<(), String> {
    id(&value.lease_id)?;
    if value.schema_version != 1
        || value.lease_id.len() > 128
        || !value
            .lease_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b":_-".contains(&b))
        || value.invocation_id.is_empty()
        || value.invocation_id.len() > 80
        || !value
            .invocation_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || !["codex", "kimi"].contains(&value.provider.as_str())
        || !["low", "medium", "high"].contains(&value.effort.as_str())
        || value.operation != "corpus-analysis"
        || value.model.trim().is_empty()
        || value.model.len() > 120
        || value.model.contains(['\n', '\r', '\0'])
        || !hash(&value.input_sha256)
        || !hash(&value.context_sha256)
        || !(1..=4 * 1024 * 1024).contains(&value.input_bytes)
        || ![1, 2].contains(&value.format_attempt)
        || value.completed_at_ms < 0
        || value.completed_at_ms as u64 > SAFE_INTEGER
    {
        return Err(INVALID.into());
    }
    validate_unique_host_ids(&value.receipt_ids, "квитанции контекста", 1000)
}
fn retrieval(value: &RetrievalReceipt, corpus: &str) -> Result<(), String> {
    id(&value.id)?;
    id(&value.policy_version)?;
    if value.schema_version != 2
        || value.snapshot_id != corpus
        || value.retrieval_version != "nativeMemoryRetrieval:1"
        || !["recall", "readEpisode"].contains(&value.operation.as_str())
        || !["lexical", "hybrid"].contains(&value.mode.as_str())
        || (value.mode == "hybrid") != value.embedding_fingerprint.is_some()
        || value.index_generation <= 0
        || value.index_generation as u64 > SAFE_INTEGER
        || !(0..=3_600_000).contains(&value.elapsed_ms)
        || value.source_bytes > 64 * 1024 * 1024
        || value.context_tokens > 1_000_000
        || value.hits.len() > 100000
    {
        return Err(INVALID.into());
    }
    if let Some(fingerprint) = &value.embedding_fingerprint {
        fingerprint.validate()?;
    }
    let mut chunks = HashSet::new();
    let mut bytes = 0usize;
    for hit in &value.hits {
        id(&hit.chunk_id)?;
        id(&hit.episode_id)?;
        if !chunks.insert(&hit.chunk_id)
            || hit.spans.len() != 1
            || !hit.fusion_score.is_finite()
            || hit.fusion_score < 0.0
            || [hit.lexical_rank, hit.semantic_rank]
                .into_iter()
                .flatten()
                .any(|r| r == 0 || r > 100000)
            || (value.operation == "recall"
                && hit.lexical_rank.is_none()
                && hit.semantic_rank.is_none())
            || (value.mode == "lexical" && hit.semantic_rank.is_some())
            || (value.operation == "readEpisode"
                && (hit.lexical_rank.is_some()
                    || hit.semantic_rank.is_some()
                    || hit.fusion_score != 0.0))
        {
            return Err(INVALID.into());
        }
        let span = &hit.spans[0];
        validate_span(span, "интервал квитанции")?;
        bytes = bytes.checked_add(span.end - span.start).ok_or(INVALID)?;
    }
    if bytes != value.source_bytes {
        return Err(INVALID.into());
    }
    Ok(())
}
pub(super) fn validate(value: &ResearchPublication) -> Result<(), String> {
    let receipt = &value.receipt;
    if value.schema_version != 1
        || !hash(&value.input_sha256)
        || receipt.schema_version != 1
        || value.retrievals.len() > 1000
        || value.model_contexts.len() > 1000
        || receipt.analyzed_spans.len() > 100000
    {
        return Err(INVALID.into());
    }
    id(&receipt.id)?;
    receipt.snapshot.validate()?;
    validate_timestamp(&receipt.published_at, "время публикации")?;
    if receipt.published_at != receipt.snapshot.created_at
        || receipt.item_ids.len() != receipt.snapshot.item_ids.len()
    {
        return Err(INVALID.into());
    }
    let mut drafts = HashSet::new();
    for (mapped, item) in receipt.item_ids.iter().zip(&receipt.snapshot.item_ids) {
        id(&mapped.draft_id)?;
        if !drafts.insert(&mapped.draft_id) || mapped.item_id != *item {
            return Err(INVALID.into());
        }
    }
    let coverage = &receipt.coverage;
    validate_limitations(&coverage.limitations, "ограничения покрытия")?;
    if coverage.loaded_documents > SAFE_INTEGER
        || coverage.loaded_bytes > SAFE_INTEGER
        || coverage.indexed_documents > coverage.loaded_documents
        || coverage.analyzed_documents > coverage.indexed_documents
        || coverage.used_documents > coverage.analyzed_documents
        || coverage.total_documents.is_some_and(|v| {
            v < coverage.loaded_documents
                || v > SAFE_INTEGER
                || (coverage.complete && v != coverage.loaded_documents)
        })
    {
        return Err(INVALID.into());
    }
    let mut retrieval_ids = HashSet::new();
    let mut analyzed_spans = Vec::new();
    let mut span_keys = HashSet::new();
    let mut documents = HashSet::new();
    let mut hit_count = 0usize;
    let mut source_bytes = 0usize;
    let mut tokens = 0usize;
    for record in &value.retrievals {
        retrieval(record, &receipt.snapshot.corpus_snapshot_id)?;
        if !retrieval_ids.insert(&record.id) {
            return Err(INVALID.into());
        }
        hit_count += record.hits.len();
        source_bytes += record.source_bytes;
        tokens += record.context_tokens;
        if hit_count > 100000 || source_bytes > 64 * 1024 * 1024 || tokens > 1_000_000 {
            return Err(INVALID.into());
        }
        for hit in &record.hits {
            let span = &hit.spans[0];
            if span_keys.insert((&span.document_id, &span.revision_id, span.start, span.end)) {
                analyzed_spans.push(span);
            }
            documents.insert(&span.document_id);
        }
    }
    if analyzed_spans != receipt.analyzed_spans.iter().collect::<Vec<_>>()
        || documents.len() as u64 != coverage.analyzed_documents
    {
        return Err(INVALID.into());
    }
    let mut leases = HashSet::new();
    let mut invocations = HashSet::new();
    let mut consumed = HashSet::new();
    let mut input_bytes = 0usize;
    for entry in &value.model_contexts {
        context(entry)?;
        if !leases.insert(&entry.lease_id) || !invocations.insert(&entry.invocation_id) {
            return Err(INVALID.into());
        }
        input_bytes += entry.input_bytes;
        if input_bytes > 64 * 1024 * 1024 {
            return Err(INVALID.into());
        }
        for receipt_id in &entry.receipt_ids {
            if !retrieval_ids.contains(receipt_id) {
                return Err(INVALID.into());
            }
            consumed.insert(receipt_id);
        }
    }
    if consumed != retrieval_ids {
        return Err(INVALID.into());
    }
    Ok(())
}
