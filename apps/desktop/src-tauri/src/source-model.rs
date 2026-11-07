use crate::connections::Provider;
use serde::{Deserialize, Serialize};
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceBudget {
    pub max_steps: u32,
    pub max_documents: u32,
    pub max_bytes: u64,
    pub max_duration_ms: u64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceGrantRequest {
    pub provider: Provider,
    pub resource_id: String,
    pub author_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub allow_model_processing: bool,
    pub operations: Vec<String>,
    pub budget: SourceBudget,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceGrant {
    #[serde(flatten)]
    pub request: SourceGrantRequest,
    pub id: String,
    pub account_id: String,
    pub snapshot_id: String,
    pub expires_at: String,
}
#[cfg_attr(test, derive(Serialize))]
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SourceAction {
    Describe,
    Coverage,
    Search {
        query: String,
        limit: usize,
    },
    Collect {
        cursor: Option<String>,
        limit: usize,
    },
    Read {
        #[serde(rename = "documentIds")]
        document_ids: Vec<String>,
    },
}
impl SourceAction {
    pub(super) fn operation(&self) -> &'static str {
        match self {
            Self::Describe => "describe",
            Self::Coverage => "coverage",
            Self::Search { .. } => "search",
            Self::Collect { .. } => "collect",
            Self::Read { .. } => "read",
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceDocument {
    pub id: String,
    pub provider: Provider,
    pub resource_id: String,
    pub author_id: Option<String>,
    pub author_label: Option<String>,
    pub authorship: String,
    pub kind: String,
    pub mode: String,
    pub text: String,
    pub source_url: Option<String>,
    pub created_at: Option<String>,
    pub retrieved_at: Option<String>,
    pub content_hash: String,
    pub reply_to: Option<String>,
    pub is_forwarded: bool,
    pub truncated: bool,
    pub partition: String,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceCoverage {
    pub documents: u64,
    pub total_documents: Option<u64>,
    pub bytes: u64,
    pub complete: bool,
    pub truncated: bool,
    pub not_analyzed: u64,
    pub unavailable: Vec<String>,
}
#[cfg_attr(test, derive(Deserialize))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceResult {
    pub snapshot_id: String,
    pub documents: Vec<SourceDocument>,
    pub coverage: SourceCoverage,
    pub cursor: Option<String>,
    pub limitations: Vec<String>,
}
#[cfg_attr(test, derive(Deserialize))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedSource {
    pub id: String,
    pub label: String,
    pub document_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Checkpoint {
    pub(super) phase: String,
    pub(super) offset: u64,
    pub(super) cursor: String,
    pub(super) index: usize,
    pub(super) queue: Vec<serde_json::Value>,
}
#[derive(Default)]
pub(crate) struct RawItem {
    pub id: String,
    pub author: Option<String>,
    pub author_label: Option<String>,
    pub text: String,
    pub kind: String,
    pub mode: String,
    pub url: Option<String>,
    pub created: Option<String>,
    pub reply_to: Option<String>,
    pub forwarded: bool,
    pub thread: Option<String>,
}
#[derive(Default)]
pub(crate) struct Page {
    pub items: Vec<RawItem>,
    pub done: bool,
    pub total: Option<u64>,
    pub not_analyzed: u64,
    pub unavailable: Vec<String>,
    pub truncated: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Record {
    pub(super) grant: SourceGrant,
    pub(super) documents: Vec<SourceDocument>,
    pub(super) coverage: SourceCoverage,
    pub(super) cursor: Option<String>,
    pub(super) checkpoint: Checkpoint,
    pub(super) steps: u32,
    pub(super) started_at: Option<i64>,
    pub(super) cancelled: bool,
    pub(super) limitations: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ImportedRecord {
    pub(super) provider: Provider,
    pub(super) documents: Vec<SourceDocument>,
    pub(super) warnings: Vec<String>,
}
