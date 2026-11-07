use super::{fingerprint::InputFingerprint, preview::PreviewResource};
use crate::corpus_database::{CorpusDatabase, RecordInput, RecordNamespace};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    io::{self, Write},
};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredPreview {
    pub schema_version: u8,
    pub id: String,
    pub provider: String,
    pub source_format: String,
    pub adapter_version: String,
    pub fingerprint: InputFingerprint,
    pub resources: Vec<PreviewResource>,
}
const INVALID: &str = "Предпросмотр повреждён или не поддерживается.";
const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_SAFE: u64 = 9_007_199_254_740_991;
// Only this native codec issues identities. Re-selecting identical input with
// identical adapter output reuses metadata and never creates new originals.
pub fn preview_id(value: &StoredPreview) -> Result<String, String> {
    let bytes = encode(&(
        value.schema_version,
        &value.provider,
        &value.source_format,
        &value.adapter_version,
        &value.fingerprint,
        &value.resources,
    ))?;
    Ok(format!("preview:{:x}", Sha256::digest(bytes)))
}
fn validate(value: &StoredPreview) -> Result<(), String> {
    if value.schema_version != 1
        || !matches!(
            value.provider.as_str(),
            "telegram" | "vk" | "github" | "instagram" | "web" | "savedPage"
        )
        || value.source_format.is_empty()
        || value.source_format.len() > 256
        || value.adapter_version.is_empty()
        || value.adapter_version.len() > 256
        || value.fingerprint.byte_length > MAX_SAFE
        || value.fingerprint.sha256.len() != 64
        || !value
            .fingerprint
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || value.resources.len() > 512
    {
        return Err(INVALID.into());
    }
    let mut ids = HashSet::new();
    let mut count = 0u64;
    for r in &value.resources {
        if r.id.trim().is_empty()
            || r.id.len() > 2048
            || r.label.len() > 65536
            || !ids.insert(&r.id)
            || r.author_ids.len() > 4096
        {
            return Err(INVALID.into());
        }
        let mut authors = HashSet::new();
        for a in &r.author_ids {
            if a.len() > 128
                || a.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
                    .is_empty()
                || a.chars().any(char::is_control)
                || !authors.insert(a)
            {
                return Err(INVALID.into());
            }
        }
        if let Some(n) = r.estimated_documents {
            count = count
                .checked_add(n)
                .filter(|n| *n <= MAX_SAFE)
                .ok_or(INVALID)?;
        }
    }
    if value.id != preview_id(value)? {
        return Err(INVALID.into());
    }
    Ok(())
}
pub fn save_preview(db: &mut CorpusDatabase, value: &StoredPreview) -> Result<(), String> {
    validate(value)?;
    if db.read(&value.id)?.is_some() {
        return if read_preview(db, &value.id)? == *value {
            Ok(())
        } else {
            Err(INVALID.into())
        };
    }
    let state_json = String::from_utf8(encode(value)?).map_err(|_| INVALID)?;
    db.save_ingestion(&RecordInput {
        id: value.id.clone(),
        kind: "import".into(),
        provider: value.provider.clone(),
        state_json,
        documents: vec![],
    })?;
    Ok(())
}
pub fn read_preview(db: &CorpusDatabase, id: &str) -> Result<StoredPreview, String> {
    if !id.starts_with("preview:") || id.len() != 72 {
        return Err(INVALID.into());
    }
    let stored = db.read(id)?.ok_or("Предпросмотр источника не найден.")?;
    if stored.namespace != RecordNamespace::Ingestion
        || stored.input.kind != "import"
        || !stored.input.documents.is_empty()
        || !stored.references.is_empty()
    {
        return Err(INVALID.into());
    }
    let value: StoredPreview =
        serde_json::from_str(&stored.input.state_json).map_err(|_| INVALID)?;
    if value.id != id || value.provider != stored.input.provider {
        return Err(INVALID.into());
    }
    validate(&value)?;
    Ok(value)
}
struct CappedBytes(Vec<u8>);
impl Write for CappedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_BYTES - self.0.len() {
            return Err(io::Error::other("Предпросмотр превышает бюджет хранения."));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let mut writer = CappedBytes(Vec::new());
    serde_json::to_writer(&mut writer, value).map_err(|_| INVALID)?;
    Ok(writer.0)
}
#[cfg(test)]
#[path = "ingestion-preview-store-tests.rs"]
mod tests;
