//! Context reservation boundary. Native owns source assembly and dispatch identity.
use super::{session::RecordedRetrieval, types::MemoryItem};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelContextRequest {
    pub schema_version: u8,
    pub purpose: String,
    #[serde(deserialize_with = "super::types::required_nullable")]
    pub manifest_id: Option<String>,
    pub system_prompt: String,
    pub task: String,
    pub receipt_ids: Vec<String>,
    pub memory_item_ids: Vec<String>,
    pub format_attempt: u8,
}
impl ModelContextRequest {
    pub fn validate(&self) -> Result<(), String> {
        let valid_id = |id: &str| {
            !id.trim().is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
        };
        let valid_ids = |ids: &[String]| {
            ids.len() <= 1000
                && ids.iter().all(|id| valid_id(id))
                && ids.iter().collect::<HashSet<_>>().len() == ids.len()
        };
        let valid_purpose = match self.purpose.as_str() {
            "research" => self.manifest_id.is_none() && self.memory_item_ids.is_empty(),
            "personaReply" => self.manifest_id.as_deref().is_some_and(valid_id),
            _ => false,
        };
        if self.schema_version != 2
            || ![1, 2].contains(&self.format_attempt)
            || self.system_prompt.len() > 40_000
            || self.task.len() > 200_000
            || !valid_ids(&self.receipt_ids)
            || !valid_ids(&self.memory_item_ids)
            || !valid_purpose
        {
            return Err("Неверный запрос контекста модели.".into());
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelContextLease {
    pub schema_version: u8,
    pub id: String,
    pub single_use: bool,
    pub persona_id: String,
    pub input_sha256: String,
    pub input_bytes: usize,
    pub system_prompt: String,
    pub task: String,
    pub corpus_snapshot_id: String,
    pub manifest_id: Option<String>,
    pub purpose: &'static str,
    pub format_attempt: u8,
    pub policy_version: String,
    pub receipt_ids: Vec<String>,
    pub items: Vec<MemoryItem>,
    pub sources: ContextSources,
    pub context_sha256: String,
    pub expires_at: String,
    pub usage: ContextUsage,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub model_calls: u32,
    pub input_bytes: u64,
}
// Share the already bounded native excerpts; serialization neither reloads nor
// fabricates sources and does not need a second owned copy of their text.
pub struct ContextSources(pub(crate) Vec<Arc<RecordedRetrieval>>);
impl Serialize for ContextSources {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq =
            serializer.serialize_seq(Some(self.0.iter().map(|r| r.sources.len()).sum()))?;
        for record in &self.0 {
            for source in &record.sources {
                seq.serialize_element(source)?;
            }
        }
        seq.end()
    }
}

#[cfg(test)]
#[path = "memory-context-wire-tests.rs"]
mod tests;
