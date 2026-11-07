use super::super::RevisionReference;
use serde::{Deserialize, Serialize};

/// Adapter-owned identity, never a guessed person or account match. Text is the
/// exact immutable decoded document text; byte spans address that text, not its
/// archive JSON envelope (the same rule as projectBoundExcerpt).
#[derive(Clone, Debug)]
pub struct EpisodeDocument {
    pub reference: RevisionReference,
    pub source_key: String,
    pub binding_id: String,
    pub conversation_id: String,
    pub thread_id: Option<String>,
    pub timestamp_ms: Option<i64>,
    pub text: String,
    /// Reply, quote, forward and verified overlap/duplicate links. Cross-source
    /// links must be resolved by acquisition, not inferred from equal names.
    pub related: Vec<RevisionReference>,
    /// An unresolved relation cannot establish reference eligibility.
    pub relations_complete: bool,
    pub holdout: bool,
}
#[derive(Clone, Debug)]
pub struct EpisodePolicy {
    pub time_gap_ms: i64,
    pub holdout_basis_points: u16,
    pub seed: String,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum Partition {
    Reference,
    Holdout,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedEpisode {
    pub schema_version: u8,
    pub id: String,
    pub snapshot_id: String,
    pub binding_id: String,
    pub conversation_id: String,
    #[serde(deserialize_with = "required_thread")]
    pub thread_id: Option<String>,
    pub document_refs: Vec<RevisionReference>,
    pub partition: Partition,
    pub split_version: String,
    pub assembly_version: String,
    pub limitations: Vec<String>,
}
/// Private construction prevents consumers from forging reference assignments.
/// The immutable borrow also prevents changing originals after the split.
pub struct PreparedEpisodes<'a> {
    pub(super) split_version: String,
    pub(super) documents: &'a [EpisodeDocument],
    pub(super) episodes: Vec<PreparedEpisode>,
    pub(super) members: Vec<Vec<usize>>,
}
impl PreparedEpisodes<'_> {
    pub fn split_version(&self) -> &str {
        &self.split_version
    }
    pub fn episodes(&self) -> &[PreparedEpisode] {
        &self.episodes
    }
}

/// The concrete local tokenizer owns counting and its pinned fingerprint.
/// Never replace this with a character/byte estimate advertised as model tokens.
pub trait EpisodeTokenizer {
    fn version(&self) -> &str;
    fn count(&self, text: &str) -> Result<usize, String>;
}
pub struct ChunkPolicy {
    pub max_tokens: usize,
    pub max_bytes: usize,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChunkSpan {
    pub document_id: String,
    pub revision_id: String,
    pub unit: SpanUnit,
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum SpanUnit {
    Utf8Bytes,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedChunk {
    pub schema_version: u8,
    pub id: String,
    pub episode_id: String,
    pub snapshot_id: String,
    pub spans: Vec<ChunkSpan>,
    pub text: String,
    pub token_count: usize,
    pub tokenizer_version: String,
    pub chunking_version: String,
    pub partition: Partition,
}

fn required_thread<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}
