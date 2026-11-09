//! Read-only, reference-partition source inspection. Native persisted grants own
//! the scope; the response excludes stored originals and model input context.
use super::{episode_read, search_workflow::QueryPeriod};
use crate::{ingestion::operations::Operation, sources::Sources};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EpisodeInspectRequest {
    pub schema_version: u8,
    pub operation_id: String,
    pub job_id: String,
    pub snapshot_id: String,
    pub persona_id: String,
    pub episode_id: String,
    pub period: QueryPeriod,
    pub expected_index_generation: i64,
    pub max_duration_ms: i64,
}
impl EpisodeInspectRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.operation_id.trim().is_empty()
            || self.operation_id.chars().any(char::is_control)
            || self.operation_id.len() > 128
            || self.job_id.len() > 128
        {
            return Err("Неверная версия или область просмотра источника.".into());
        }
        self.input().validate()
    }
    fn input(&self) -> episode_read::EpisodeReadInput<'_> {
        episode_read::EpisodeReadInput {
            job_id: &self.job_id,
            snapshot_id: &self.snapshot_id,
            persona_id: &self.persona_id,
            episode_id: &self.episode_id,
            period: crate::corpus_database::search::SearchPeriod {
                from: self.period.from,
                to: self.period.to,
                include_undated: self.period.include_undated,
            },
            expected_index_generation: Some(self.expected_index_generation),
            max_chunks: 100,
            max_source_bytes: 1024 * 1024,
            max_stored_bytes: 16 * 1024 * 1024,
            max_context_tokens: 1000000,
            max_duration_ms: self.max_duration_ms,
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeInspectChunk {
    pub chunk_id: String,
    pub excerpt: super::source_excerpt::SourceExcerpt,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeInspectResult {
    pub schema_version: u8,
    pub snapshot_id: String,
    pub index_generation: i64,
    pub persona_id: String,
    pub episode_id: String,
    pub source_bytes: usize,
    pub context_tokens: usize,
    pub chunks: Vec<EpisodeInspectChunk>,
}
pub fn inspect(
    sources: &Sources,
    operation: &Operation,
    request: &EpisodeInspectRequest,
    now: impl Fn() -> i64,
) -> Result<EpisodeInspectResult, String> {
    request.validate()?;
    operation.check()?;
    if operation.id() != request.operation_id {
        return Err("Запрос относится к другой операции просмотра.".into());
    }
    let value = episode_read::read_for_display(sources, operation, &request.input(), now)?;
    if value.entries.is_empty() || value.entries.len() != value.excerpts.len() {
        return Err("Эпизод не содержит проверенных фрагментов.".into());
    }
    let result = EpisodeInspectResult {
        schema_version: 1,
        snapshot_id: value.source.snapshot_id,
        index_generation: value.source.index_generation,
        persona_id: request.persona_id.clone(),
        episode_id: value.episode.id.clone(),
        source_bytes: value.source_bytes,
        context_tokens: value.context_tokens,
        chunks: value
            .entries
            .into_iter()
            .zip(value.excerpts)
            .map(|(entry, excerpt)| EpisodeInspectChunk {
                chunk_id: entry.chunk.id.clone(),
                excerpt,
            })
            .collect(),
    };
    operation.check()?;
    Ok(result)
}
