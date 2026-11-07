//! Versioned own schemas over the unchanged encrypted corpus container.
//! Provider checkpoint payloads are opaque values, never recursively renamed.
use super::model::{
    Checkpoint, ImportedRecord, Record, SourceBudget, SourceCoverage, SourceDocument, SourceGrant,
    SourceGrantRequest,
};
use crate::connections::Provider;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

pub(super) const FORMAT: &str = "proteus-source-record";
pub(super) const VERSION: u8 = 3;
pub(super) const INVALID: &str = "Формат сохранённого корпуса повреждён или не поддерживается.";
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    format: String,
    version: u8,
    kind: String,
    payload: T,
}
pub(super) trait StoredSource: Serialize + DeserializeOwned {
    const KIND: &'static str;
    fn decode_legacy(value: Value) -> Result<Self, String>;
    fn decode_current(value: Value) -> Result<Self, String> {
        serde_json::from_value(value).map_err(|_| INVALID.into())
    }
    fn decode_v2(value: Value) -> Result<Self, String> {
        Self::decode_current(value)
    }
}
pub(super) fn current<T: StoredSource>(value: Value) -> Result<T, String> {
    let envelope: Envelope<Value> = serde_json::from_value(value).map_err(|_| INVALID)?;
    if envelope.format != FORMAT || !matches!(envelope.version, 2 | 3) || envelope.kind != T::KIND {
        return Err(INVALID.into());
    }
    if envelope.version == 2 {
        T::decode_v2(envelope.payload)
    } else {
        T::decode_current(envelope.payload)
    }
}
pub(super) fn encode<T: StoredSource>(value: &T) -> Result<Value, String> {
    let encoded = serde_json::to_value(Envelope {
        format: FORMAT.into(),
        version: VERSION,
        kind: T::KIND.into(),
        payload: value,
    })
    .map_err(|_| INVALID)?;
    let decoded: T = current(encoded.clone())?;
    // Full typed roundtrip checks text, links, hashes, counts and opaque queue values.
    if serde_json::to_value(decoded).map_err(|_| INVALID)? != encoded["payload"] {
        return Err(INVALID.into());
    }
    Ok(encoded)
}
pub(super) fn normalize(value: Value, is_current: bool) -> Result<Value, String> {
    if is_current {
        match value["kind"].as_str() {
            Some("grant") => encode(&current::<Record>(value)?),
            Some("import") => encode(&current::<ImportedRecord>(value)?),
            Some("deleted") => encode(&current::<Deleted>(value)?),
            _ => Err(INVALID.into()),
        }
    } else if value.get("version").is_some() {
        encode(&Record::decode_legacy(value)?)
    } else {
        encode(&ImportedRecord::decode_legacy(value)?)
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Deleted {
    pub(super) provider: Provider,
}
impl StoredSource for Deleted {
    const KIND: &'static str = "deleted";
    fn decode_legacy(_: Value) -> Result<Self, String> {
        Err(INVALID.into())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct LegacyBudget {
    max_steps: u32,
    max_documents: u32,
    max_bytes: u64,
    max_duration_ms: u64,
}
impl From<LegacyBudget> for SourceBudget {
    fn from(v: LegacyBudget) -> Self {
        Self {
            max_steps: v.max_steps,
            max_documents: v.max_documents,
            max_bytes: v.max_bytes,
            max_duration_ms: v.max_duration_ms,
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct LegacyGrant {
    provider: Provider,
    resource_id: String,
    author_id: Option<String>,
    from: Option<String>,
    to: Option<String>,
    allow_model_processing: bool,
    operations: Vec<String>,
    budget: LegacyBudget,
    id: String,
    account_id: String,
    snapshot_id: String,
    expires_at: String,
}
impl From<LegacyGrant> for SourceGrant {
    fn from(v: LegacyGrant) -> Self {
        Self {
            request: SourceGrantRequest {
                provider: v.provider,
                resource_id: v.resource_id,
                author_id: v.author_id,
                from: v.from,
                to: v.to,
                allow_model_processing: v.allow_model_processing,
                operations: v.operations,
                budget: v.budget.into(),
            },
            id: v.id,
            account_id: v.account_id,
            snapshot_id: v.snapshot_id,
            expires_at: v.expires_at,
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct LegacyDocument {
    id: String,
    provider: Provider,
    resource_id: String,
    author_id: Option<String>,
    author_label: Option<String>,
    authorship: String,
    kind: String,
    mode: String,
    text: String,
    source_url: Option<String>,
    created_at: Option<String>,
    retrieved_at: Option<String>,
    content_hash: String,
    reply_to: Option<String>,
    is_forwarded: bool,
    truncated: bool,
    partition: String,
}
impl From<LegacyDocument> for SourceDocument {
    fn from(v: LegacyDocument) -> Self {
        Self {
            id: v.id,
            provider: v.provider,
            resource_id: v.resource_id,
            author_id: v.author_id,
            author_label: v.author_label,
            authorship: v.authorship,
            kind: v.kind,
            mode: v.mode,
            text: v.text,
            source_url: v.source_url,
            created_at: v.created_at,
            retrieved_at: v.retrieved_at,
            content_hash: v.content_hash,
            reply_to: v.reply_to,
            is_forwarded: v.is_forwarded,
            truncated: v.truncated,
            partition: v.partition,
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct LegacyCoverage {
    documents: u64,
    total_documents: Option<u64>,
    bytes: u64,
    complete: bool,
    truncated: bool,
    not_analyzed: u64,
    unavailable: Vec<String>,
}
impl From<LegacyCoverage> for SourceCoverage {
    fn from(v: LegacyCoverage) -> Self {
        Self {
            documents: v.documents,
            total_documents: v.total_documents,
            bytes: v.bytes,
            complete: v.complete,
            truncated: v.truncated,
            not_analyzed: v.not_analyzed,
            unavailable: v.unavailable,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyRecord {
    version: u8,
    grant: LegacyGrant,
    documents: Vec<LegacyDocument>,
    coverage: LegacyCoverage,
    cursor: Option<String>,
    checkpoint: Checkpoint,
    steps: u32,
    started_at: Option<i64>,
    cancelled: bool,
    limitations: Vec<String>,
}
impl StoredSource for Record {
    const KIND: &'static str = "grant";
    fn decode_v2(mut value: Value) -> Result<Self, String> {
        let fields = value.as_object_mut().ok_or(INVALID)?;
        if fields.contains_key("binding") {
            return Err(INVALID.into());
        }
        fields.insert("binding".into(), Value::Null);
        Self::decode_current(value)
    }
    fn decode_current(value: Value) -> Result<Self, String> {
        if value.get("binding").is_none() {
            return Err(INVALID.into());
        }
        // Serde flatten does not reject the outer grant's unknown fields through
        // SourceGrantRequest. Validate this exact flat schema before deserializing.
        let grant = value
            .get("grant")
            .and_then(Value::as_object)
            .ok_or(INVALID)?;
        if grant.keys().any(|key| {
            ![
                "provider",
                "resourceId",
                "authorId",
                "from",
                "to",
                "allowModelProcessing",
                "operations",
                "budget",
                "id",
                "accountId",
                "snapshotId",
                "expiresAt",
            ]
            .contains(&key.as_str())
        }) {
            return Err(INVALID.into());
        }
        serde_json::from_value(value).map_err(|_| INVALID.into())
    }
    fn decode_legacy(value: Value) -> Result<Self, String> {
        let v: LegacyRecord = serde_json::from_value(value).map_err(|_| INVALID)?;
        if v.version != 1 {
            return Err(INVALID.into());
        }
        Ok(Self {
            binding: None,
            grant: v.grant.into(),
            documents: v.documents.into_iter().map(Into::into).collect(),
            coverage: v.coverage.into(),
            cursor: v.cursor,
            checkpoint: v.checkpoint,
            steps: v.steps,
            started_at: v.started_at,
            cancelled: v.cancelled,
            limitations: v.limitations,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyImport {
    provider: Provider,
    documents: Vec<LegacyDocument>,
    warnings: Vec<String>,
}
impl StoredSource for ImportedRecord {
    const KIND: &'static str = "import";
    fn decode_legacy(value: Value) -> Result<Self, String> {
        let v: LegacyImport = serde_json::from_value(value).map_err(|_| INVALID)?;
        Ok(Self {
            provider: v.provider,
            documents: v.documents.into_iter().map(Into::into).collect(),
            warnings: v.warnings,
        })
    }
}
