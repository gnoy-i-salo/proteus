//! Native storage values; the source codec owns semantic schema validation.
use super::provenance::SourceProvenance;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevisionInput {
    pub source_key: String,
    pub document_id: String,
    pub document_json: String,
    pub provenance: SourceProvenance,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordInput {
    pub id: String,
    pub kind: String,
    pub provider: String,
    pub state_json: String,
    pub documents: Vec<RevisionInput>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevisionReference {
    pub document_id: String,
    pub revision_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredRevision {
    pub reference: RevisionReference,
    pub source_key: String,
    pub document_json: String,
    pub provenance: SourceProvenance,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordGeneration {
    pub id: String,
    pub generation: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredRecord {
    pub input: RecordInput,
    pub generation: i64,
    pub references: Vec<RevisionReference>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub id: String,
    pub corpus_id: String,
    pub publication_id: String,
    pub generation: i64,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub records: Vec<RecordGeneration>,
    pub documents: Vec<RevisionReference>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationAudit {
    pub source_generation: u8,
    pub ciphertext_sha256: String,
    pub document_count: usize,
    pub canonical_sha256: String,
}
pub fn digest(value: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(value.as_ref()))
}
pub fn revision_id(input: &RevisionInput) -> String {
    format!(
        "revision:{}",
        digest(
            serde_json::to_vec(&(&input.source_key, &input.document_json, &input.provenance))
                .expect("serializing strings cannot fail")
        )
    )
}
pub fn record_hash(input: &RecordInput) -> String {
    digest(
        serde_json::to_vec(&(
            input.id.as_str(),
            input.kind.as_str(),
            input.provider.as_str(),
            input.state_json.as_str(),
            input
                .documents
                .iter()
                .map(|doc| {
                    (
                        &doc.source_key,
                        &doc.document_id,
                        &doc.document_json,
                        &doc.provenance,
                    )
                })
                .collect::<Vec<_>>(),
        ))
        .expect("serializing strings cannot fail"),
    )
}

pub fn revision_hash(input: &RevisionInput) -> String {
    digest(
        serde_json::to_vec(&(&input.document_json, &input.provenance))
            .expect("serializing provenance cannot fail"),
    )
}
