//! Verifies source support against native completed-context receipts. This is
//! not publication or an independent grant: the session owns both authorities.
use crate::{
    corpus_database::{episodes::ChunkSpan, JobRole},
    ingestion::source_excerpt::SourceExcerpt,
    memory::{
        session::RecordedRetrieval,
        types::{MemoryItem, MemoryItemKind, MemoryItemSupport},
    },
    sources::{RelationKind, SpeakerRelation},
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
pub struct VerifiedResearchSupport {
    pub analyzed_spans: Vec<ChunkSpan>,
    pub used_spans: Vec<ChunkSpan>,
}
struct BoundSource<'a> {
    source: &'a SourceExcerpt,
    episode_id: &'a str,
}
type SourceIndex<'a> = HashMap<(&'a str, &'a str), Vec<BoundSource<'a>>>;
const INVALID: &str = "Основания памяти не соответствуют завершённому анализу источников.";
fn span_key(span: &ChunkSpan) -> (&str, &str, usize, usize) {
    (&span.document_id, &span.revision_id, span.start, span.end)
}
fn own_words(source: &SourceExcerpt) -> bool {
    source.can_support_own_words
        && source.role == JobRole::Own
        && source.speaker_relation == SpeakerRelation::Target
        && source.document.authorship == "owner"
        && !source.document.is_forwarded
        && !source
            .relations
            .iter()
            .any(|relation| matches!(relation.kind, RelationKind::Quote | RelationKind::Forward))
}
fn index_sources<'a>(
    receipts: &'a [Arc<RecordedRetrieval>],
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(SourceIndex<'a>, Vec<ChunkSpan>), String> {
    let mut index: SourceIndex<'a> = HashMap::new();
    let mut spans = Vec::new();
    let mut seen = HashSet::new();
    let mut source_count = 0usize;
    for record in receipts {
        check()?;
        if record.receipt.schema_version != 2 || record.sources.len() != record.receipt.hits.len() {
            return Err(INVALID.into());
        }
        source_count = source_count
            .checked_add(record.sources.len())
            .ok_or(INVALID)?;
        if source_count > 100000 {
            return Err(INVALID.into());
        }
        for (source, hit) in record.sources.iter().zip(&record.receipt.hits) {
            if source.span.start >= source.span.end
                || source.span.end - source.span.start != source.text.len()
                || source.document.id != source.span.document_id
                || source.document.partition != "reference"
                || hit.spans.as_slice() != std::slice::from_ref(&source.span)
                || hit.episode_id.is_empty()
            {
                return Err(INVALID.into());
            }
            if seen.insert(span_key(&source.span)) {
                spans.push(source.span.clone());
            }
            index
                .entry((&source.span.document_id, &source.span.revision_id))
                .or_default()
                .push(BoundSource {
                    source,
                    episode_id: &hit.episode_id,
                });
        }
    }
    check()?;
    Ok((index, spans))
}
fn check_item(
    item: &MemoryItem,
    index: &SourceIndex<'_>,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    item.validate()?;
    if item.kind == MemoryItemKind::ReportedAssertion && item.support == MemoryItemSupport::Observed
    {
        return Err(INVALID.into());
    }
    let requires_own =
        item.kind == MemoryItemKind::StyleExample || item.support == MemoryItemSupport::Observed;
    let mut exact = false;
    let mut episodes = Vec::new();
    for span in &item.source_spans {
        check()?;
        let candidates = index
            .get(&(span.document_id.as_str(), span.revision_id.as_str()))
            .ok_or(INVALID)?;
        let mut matched = None;
        for bound in candidates {
            let source = bound.source;
            if source.span.start > span.start || source.span.end < span.end {
                continue;
            }
            let slice = source
                .text
                .get(span.start - source.span.start..span.end - source.span.start)
                .ok_or(INVALID)?;
            if requires_own && !own_words(source) {
                return Err(INVALID.into());
            }
            let current = (bound.episode_id, slice);
            if matched.is_some_and(|previous| previous != current) {
                return Err(INVALID.into());
            }
            matched = Some(current);
        }
        let (episode, slice) = matched.ok_or(INVALID)?;
        exact |= slice == item.text;
        if !episodes.contains(&episode) {
            episodes.push(episode);
        }
    }
    if (requires_own && !exact)
        || !episodes
            .iter()
            .copied()
            .eq(item.episode_ids.iter().map(String::as_str))
    {
        return Err(INVALID.into());
    }
    check()
}
pub(crate) fn verify(
    items: &[MemoryItem],
    receipts: &[Arc<RecordedRetrieval>],
    check: &dyn Fn() -> Result<(), String>,
) -> Result<VerifiedResearchSupport, String> {
    check()?;
    if items.len() > 1000 || receipts.len() > 1000 {
        return Err(INVALID.into());
    }
    let (index, analyzed_spans) = index_sources(receipts, check)?;
    let mut used_spans = Vec::new();
    let mut seen_spans = HashSet::new();
    let mut seen_items = HashSet::new();
    for item in items {
        check()?;
        if !seen_items.insert(&item.id)
            || receipts.iter().any(|record| {
                record.receipt.snapshot_id != item.snapshot_id
                    || record.receipt.policy_version != item.policy_version
            })
        {
            return Err(INVALID.into());
        }
        check_item(item, &index, check)?;
        for span in &item.source_spans {
            if seen_spans.insert(span_key(span)) {
                used_spans.push(span.clone());
            }
        }
    }
    check()?;
    Ok(VerifiedResearchSupport {
        analyzed_spans,
        used_spans,
    })
}
