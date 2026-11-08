use serde::{Deserialize, Serialize};
/// Shared admission bound for a durable selection and its native search scope.
pub const MAX_JOB_BINDINGS: usize = 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    Queued,
    Running,
    Paused,
    Cancelled,
    Failed,
    Completed,
}
impl JobStatus {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Completed => "completed",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobPhase {
    Reading,
    Normalizing,
    Episodes,
    Indexing,
    Publishing,
}
impl JobPhase {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Reading => "reading",
            Self::Normalizing => "normalizing",
            Self::Episodes => "episodes",
            Self::Indexing => "indexing",
            Self::Publishing => "publishing",
        }
    }
    pub(super) fn order(self) -> u8 {
        match self {
            Self::Reading => 0,
            Self::Normalizing => 1,
            Self::Episodes => 2,
            Self::Indexing => 3,
            Self::Publishing => 4,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobProvider {
    Telegram,
    Vk,
    Instagram,
    Github,
    Web,
    SavedPage,
}
impl JobProvider {
    pub fn key(self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Vk => "vk",
            Self::Instagram => "instagram",
            Self::Github => "github",
            Self::Web => "web",
            Self::SavedPage => "savedPage",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobRole {
    Own,
    Context,
    About,
    Reference,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobPeriod {
    pub from: Option<String>,
    pub to: Option<String>,
    pub include_undated: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobBinding {
    pub schema_version: u8,
    pub id: String,
    pub persona_id: String,
    pub provider: JobProvider,
    pub account_id: Option<String>,
    pub resource_id: String,
    pub grant_id: String,
    pub policy_version: String,
    pub role: JobRole,
    pub target_author_id: Option<String>,
    pub allowed_context_author_ids: Vec<String>,
    pub allow_unknown_context: bool,
    pub period: JobPeriod,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobSelection {
    pub preview_id: String,
    pub resource_ids: Vec<String>,
    pub bindings: Vec<JobBinding>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobBudget {
    pub max_input_bytes: i64,
    pub max_expanded_bytes: i64,
    pub max_documents: i64,
    pub max_batch_documents: i64,
    pub max_batch_bytes: i64,
    pub max_duration_ms: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobCheckpoint {
    pub adapter_version: String,
    pub cursor: Option<String>,
    pub committed_batch: i64,
    pub staging_generation: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobCoverage {
    pub loaded_documents: i64,
    pub indexed_documents: i64,
    pub analyzed_documents: i64,
    pub used_documents: i64,
    pub total_documents: Option<i64>,
    pub loaded_bytes: i64,
    pub complete: bool,
    pub limitations: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobState {
    pub selection: JobSelection,
    pub budget: JobBudget,
    pub checkpoint: JobCheckpoint,
    pub coverage: JobCoverage,
    pub published_snapshot_id: Option<String>,
    pub error: Option<JobError>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobInput {
    pub adapter_version: String,
    pub id: String,
    pub selection: JobSelection,
    pub budget: JobBudget,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredJob {
    pub id: String,
    pub status: JobStatus,
    pub phase: JobPhase,
    pub state: JobState,
    pub hash: String,
}
/// One bounded unit of durable progress. The worker stages exactly the records it
/// loaded in this batch. Storage independently measures retained UTF-8 payloads
/// against byte budgets; loaded_bytes remains the adapter-reported coverage.
/// The acquisition worker separately enforces input bytes and elapsed time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobBatch {
    pub phase: Option<JobPhase>,
    pub checkpoint: JobCheckpoint,
    pub loaded_documents: i64,
    pub loaded_bytes: i64,
    pub indexed_documents: i64,
    pub analyzed_documents: i64,
    pub used_documents: i64,
    pub total_documents: Option<i64>,
    pub complete: bool,
    pub limitations: Vec<String>,
    pub record_ids: Vec<String>,
}
