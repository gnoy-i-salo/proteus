//! Versioned own schemas over the unchanged encrypted corpus container.
//! Provider checkpoint payloads are opaque values, never recursively renamed.
use super::model::{
    Checkpoint, ImportedRecord, Record, SourceBudget, SourceCoverage, SourceDocument, SourceGrant,
    SourceGrantRequest,
};
use crate::connections::{
    storage::{EncryptedStore, SecretStore},
    Provider,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

const FORMAT: &str = "proteus-source-record";
const VERSION: u8 = 2;
const INVALID: &str = "Формат сохранённого корпуса повреждён или не поддерживается.";
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
}
fn current<T: StoredSource>(value: Value) -> Result<T, String> {
    let envelope: Envelope<Value> = serde_json::from_value(value).map_err(|_| INVALID)?;
    if envelope.format != FORMAT || envelope.version != VERSION || envelope.kind != T::KIND {
        return Err(INVALID.into());
    }
    T::decode_current(envelope.payload)
}
fn encode<T: StoredSource>(value: &T) -> Result<Value, String> {
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
pub(super) fn current_path(legacy: &Path) -> Result<PathBuf, String> {
    Ok(legacy
        .parent()
        .ok_or(INVALID)?
        .join("v2")
        .join(legacy.file_name().ok_or(INVALID)?))
}
pub(super) struct SourceStore(EncryptedStore);
impl SourceStore {
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self(EncryptedStore::new(secrets))
    }
    fn active(&self, legacy: &Path) -> Result<(Value, bool), String> {
        let path = current_path(legacy)?;
        match std::fs::symlink_metadata(&path) {
            Ok(_) => self.0.read(&path).map(|value| (value, true)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.0.read(legacy).map(|value| (value, false))
            }
            Err(_) => Err("Не удалось открыть новое поколение корпуса.".into()),
        }
    }
    pub fn read<T: StoredSource>(&self, legacy: &Path) -> Result<T, String> {
        let (value, is_current) = self.active(legacy)?;
        if is_current {
            current(value)
        } else {
            T::decode_legacy(value)
        }
    }
    pub fn write<T: StoredSource>(&self, legacy: &Path, value: &T) -> Result<(), String> {
        let encoded = encode(value)?;
        self.0.write(&current_path(legacy)?, &encoded)
    }
    pub fn owner(&self, legacy: &Path) -> Result<Provider, String> {
        let (value, is_current) = self.active(legacy)?;
        if is_current {
            match value["kind"].as_str() {
                Some("grant") => {
                    current::<Record>(value).map(|record| record.grant.request.provider)
                }
                Some("import") => current::<ImportedRecord>(value).map(|record| record.provider),
                Some("deleted") => current::<Deleted>(value).map(|record| record.provider),
                _ => Err(INVALID.into()),
            }
        } else if value.get("version").is_some() {
            Record::decode_legacy(value).map(|record| record.grant.request.provider)
        } else {
            ImportedRecord::decode_legacy(value).map(|record| record.provider)
        }
    }
    pub fn is_deleted(&self, legacy: &Path) -> Result<bool, String> {
        let (value, is_current) = self.active(legacy)?;
        if is_current && value["kind"] == "deleted" {
            let _: Deleted = current(value)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn remove(&self, legacy: &Path) -> Result<(), String> {
        // Persist revocation before either unlink. An interrupted deletion must not
        // reactivate the preserved legacy generation after restart.
        let provider = self.owner(legacy)?;
        self.write(legacy, &Deleted { provider })?;
        super::corpus::remove_file(legacy)?;
        super::corpus::remove_file(&current_path(legacy)?)
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Deleted {
    provider: Provider,
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
    fn decode_current(value: Value) -> Result<Self, String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connections::storage::{random_id, MemorySecrets};
    use serde_json::json;
    fn legacy() -> Value {
        let text = "\r\n  Ответ é 😀\t";
        json!({"version":1,"grant":{"provider":"telegram","resource-id":"chat:1","author-id":"a:1","from":null,"to":null,
            "allow-model-processing":true,"operations":["read"],"budget":{"max-steps":12,"max-documents":20,"max-bytes":1048576,"max-duration-ms":12000},
            "id":"grant-fixture-1234","account-id":"account:1","snapshot-id":"snap:1","expires-at":"2030-01-01T00:00:00Z"},
            "documents":[{"id":"d:1","provider":"telegram","resource-id":"chat:1","author-id":null,"author-label":"  Автор é  ",
            "authorship":"unknown","kind":"message","mode":"dialogue","text":text,"source-url":null,"created-at":null,"retrieved-at":"2025-01-01T00:00:00Z",
            "content-hash":"unchanged-hash","reply-to":"d:0","is-forwarded":true,"truncated":false,"partition":"reference"}],
            "coverage":{"documents":1,"total-documents":null,"bytes":text.len(),"complete":false,"truncated":false,"not-analyzed":2,"unavailable":["attachment"]},
            "cursor":"opaque==","checkpoint":{"phase":"messages","offset":23,"cursor":"a-b_c","index":4,
            "queue":[{"author_id":7,"reply-to":"raw-external","publicUrls":"opaque","nested":{"max-steps":900},"text":text}]},
            "steps":3,"started_at":12345,"cancelled":true,"limitations":["  unchanged\r\n"]})
    }
    fn store() -> (SourceStore, PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("proteus-generation-{}", random_id(18).unwrap()));
        (
            SourceStore::new(Arc::new(MemorySecrets::default())),
            directory.join("grant-fixture-1234.enc"),
        )
    }
    #[test]
    fn legacy_to_current_preserves_original_exact_metadata_and_opaque_queue() {
        let (store, path) = store();
        let old = legacy();
        store.0.write(&path, &old).unwrap();
        let original = std::fs::read(&path).unwrap();
        let record: Record = store.read(&path).unwrap();
        assert_eq!(record.documents[0].text, "\r\n  Ответ é 😀\t");
        assert_eq!(record.documents[0].authorship, "unknown");
        assert_eq!(record.documents[0].reply_to.as_deref(), Some("d:0"));
        assert_eq!(record.started_at, Some(12345));
        assert_eq!(record.checkpoint.queue[0], old["checkpoint"]["queue"][0]);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        store.write(&path, &record).unwrap();
        store.write(&path, &record).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let current: Value = store.0.read(&current_path(&path).unwrap()).unwrap();
        assert_eq!(current["version"], 2);
        assert_eq!(current["kind"], "grant");
        assert_eq!(current["payload"]["startedAt"], 12345);
        assert!(current["payload"].get("started_at").is_none());
        let restored: Record = store.read(&path).unwrap();
        assert_eq!(
            serde_json::to_value(restored).unwrap(),
            serde_json::to_value(record).unwrap()
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn rejects_corrupt_unknown_or_wrong_kind_current_without_legacy_fallback() {
        let (store, path) = store();
        store.0.write(&path, &legacy()).unwrap();
        let record: Record = store.read(&path).unwrap();
        store.write(&path, &record).unwrap();
        let current_path = current_path(&path).unwrap();
        std::fs::write(&current_path, b"corrupt").unwrap();
        assert!(store.read::<Record>(&path).is_err());
        for field in ["version", "kind", "format"] {
            let mut value = encode(&record).unwrap();
            value[field] = json!("unknown");
            store.0.write(&current_path, &value).unwrap();
            assert!(store.read::<Record>(&path).is_err());
        }
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn rejects_legacy_mixed_keys_and_unknowns_but_not_external_queue_names() {
        for pointer in [
            "/grant/resourceId",
            "/grant/budget/maxSteps",
            "/documents/0/authorId",
            "/coverage/totalDocuments",
            "/startedAt",
            "/unknown",
        ] {
            let mut value = legacy();
            let (parent, key) = pointer.rsplit_once('/').unwrap();
            value
                .pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(key.into(), json!(null));
            assert!(Record::decode_legacy(value).is_err(), "{pointer}");
        }
        assert!(Record::decode_legacy(legacy()).is_ok());
        let mut value = legacy();
        value["version"] = json!(99);
        assert!(Record::decode_legacy(value).is_err());
    }
    #[test]
    fn current_payload_rejects_unknown_and_legacy_nested_fields() {
        let record = Record::decode_legacy(legacy()).unwrap();
        for pointer in [
            "/payload/grant/resource-id",
            "/payload/grant/unknown",
            "/payload/documents/0/author-id",
            "/payload/coverage/not-analyzed",
            "/payload/checkpoint/unknown",
        ] {
            let mut value = encode(&record).unwrap();
            let (parent, key) = pointer.rsplit_once('/').unwrap();
            value
                .pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(key.into(), json!(null));
            assert!(current::<Record>(value).is_err(), "{pointer}");
        }
    }
    #[test]
    fn imported_records_have_distinct_kind_and_preserve_documents_without_grants() {
        let (store, path) = store();
        let value = legacy();
        let old = json!({"provider":"telegram","documents":value["documents"],"warnings":["  warning\r\n"]});
        store.0.write(&path, &old).unwrap();
        let record: ImportedRecord = store.read(&path).unwrap();
        store.write(&path, &record).unwrap();
        assert!(store.read::<Record>(&path).is_err());
        assert_eq!(store.owner(&path).unwrap(), Provider::Telegram);
        let restored: ImportedRecord = store.read(&path).unwrap();
        assert_eq!(restored.documents[0].text, "\r\n  Ответ é 😀\t");
        assert_eq!(restored.warnings, ["  warning\r\n"]);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn failed_generation_publish_keeps_original_readable() {
        let (store, path) = store();
        store.0.write(&path, &legacy()).unwrap();
        let original = std::fs::read(&path).unwrap();
        let record: Record = store.read(&path).unwrap();
        std::fs::write(path.parent().unwrap().join("v2"), b"blocked directory").unwrap();
        assert!(store.write(&path, &record).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_file(path.parent().unwrap().join("v2")).unwrap();
        assert!(store.read::<Record>(&path).is_ok());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn delete_removes_both_generations_and_interrupted_delete_stays_revoked() {
        let (store, path) = store();
        store.0.write(&path, &legacy()).unwrap();
        let record: Record = store.read(&path).unwrap();
        store.write(&path, &record).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(store.remove(&path).is_err());
        assert!(store.read::<Record>(&path).is_err());
        assert_eq!(store.owner(&path).unwrap(), Provider::Telegram);
        std::fs::remove_dir(&path).unwrap();
        store.0.write(&path, &legacy()).unwrap();
        store.remove(&path).unwrap();
        assert!(!path.exists());
        assert!(!current_path(&path).unwrap().exists());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
