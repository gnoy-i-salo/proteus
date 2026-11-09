//! Durable native publication metadata. Validation proves internal consistency,
//! not authorization or truth; the session derives this from completed contexts.
use super::{
    session::{CompletedModelContext, RetrievalReceipt},
    types::MemorySnapshot,
};
use crate::corpus_database::episodes::ChunkSpan;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicationCoverage {
    pub loaded_documents: u64,
    pub indexed_documents: u64,
    pub analyzed_documents: u64,
    pub used_documents: u64,
    #[serde(deserialize_with = "super::types::required_nullable")]
    pub total_documents: Option<u64>,
    pub loaded_bytes: u64,
    pub complete: bool,
    pub limitations: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishedItemId {
    pub draft_id: String,
    pub item_id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchPublicationReceipt {
    pub schema_version: u8,
    pub id: String,
    pub snapshot: MemorySnapshot,
    pub item_ids: Vec<PublishedItemId>,
    pub coverage: PublicationCoverage,
    pub analyzed_spans: Vec<ChunkSpan>,
    pub published_at: String,
}
/// Receipts contain IDs, spans, scores and input hashes. Original source text
/// and prompts remain with their existing owners, not duplicated in this audit.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchPublication {
    pub schema_version: u8,
    /// Hash of the exact canonical native publication draft for replay identity.
    pub input_sha256: String,
    pub receipt: ResearchPublicationReceipt,
    pub retrievals: Vec<RetrievalReceipt>,
    pub model_contexts: Vec<CompletedModelContext>,
}
impl ResearchPublication {
    pub fn validate(&self) -> Result<(), String> {
        validation::validate(self)
    }
}

#[cfg(test)]
#[path = "memory-publication-tests.rs"]
mod tests;

#[path = "memory-publication-validation.rs"]
mod validation;
