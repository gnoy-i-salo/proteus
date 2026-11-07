//! Structural provenance only. IDs and original bytes must come from an acquisition
//! adapter; SourceDocument.id and UTF-8 text size are not substitutes for them.
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;

const INVALID: &str = "Неверные сведения о происхождении документа.";
const LEGACY_FORMAT: &str = "proteusEncryptedSourceRecord";
const LEGACY_ADAPTER: &str = "legacy-source-record-";
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Acquisition {
    Api,
    Archive,
    SelectedUrl,
    SavedPage,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HashAlgorithm {
    Sha256,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ProvenanceLimitation {
    OriginalObjectIdUnavailable,
    OriginalBytesUnavailable,
    AttachmentsUnavailable,
    RelationsIncomplete,
}
const ALL_LIMITATIONS: [ProvenanceLimitation; 4] = [
    ProvenanceLimitation::OriginalObjectIdUnavailable,
    ProvenanceLimitation::OriginalBytesUnavailable,
    ProvenanceLimitation::AttachmentsUnavailable,
    ProvenanceLimitation::RelationsIncomplete,
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceProvenance {
    pub acquisition: Acquisition,
    pub adapter_version: String,
    pub source_format: String,
    #[serde(deserialize_with = "required_nullable")]
    pub original_object_id: Option<String>,
    #[serde(deserialize_with = "required_byte_length")]
    pub original_byte_length: Option<u64>,
    pub hash_algorithm: HashAlgorithm,
    pub limitations: Vec<ProvenanceLimitation>,
}
impl SourceProvenance {
    pub fn validate(&self) -> Result<(), String> {
        let limitations: HashSet<_> = self.limitations.iter().copied().collect();
        if self.adapter_version.is_empty()
            || self.source_format.is_empty()
            || limitations.len() != self.limitations.len()
            || self
                .original_object_id
                .as_ref()
                .is_some_and(String::is_empty)
            || self
                .original_byte_length
                .is_some_and(|size| size > MAX_SAFE_INTEGER)
            || self.original_object_id.is_none()
                != limitations.contains(&ProvenanceLimitation::OriginalObjectIdUnavailable)
            || self.original_byte_length.is_none()
                != limitations.contains(&ProvenanceLimitation::OriginalBytesUnavailable)
        {
            return Err(INVALID.into());
        }
        let legacy =
            self.source_format == LEGACY_FORMAT || self.adapter_version.starts_with(LEGACY_ADAPTER);
        if legacy
            && (self.source_format != LEGACY_FORMAT
                || !self
                    .adapter_version
                    .strip_prefix(LEGACY_ADAPTER)
                    .is_some_and(|version| matches!(version, "v0" | "v1" | "v2" | "v3"))
                || self.original_object_id.is_some()
                || self.original_byte_length.is_some()
                || ALL_LIMITATIONS
                    .iter()
                    .any(|limitation| !limitations.contains(limitation)))
        {
            return Err(INVALID.into());
        }
        Ok(())
    }
    pub fn legacy(acquisition: Acquisition, generation: u8) -> Result<Self, String> {
        let provenance = Self {
            acquisition,
            adapter_version: format!("{LEGACY_ADAPTER}v{generation}"),
            source_format: LEGACY_FORMAT.into(),
            original_object_id: None,
            original_byte_length: None,
            hash_algorithm: HashAlgorithm::Sha256,
            limitations: ALL_LIMITATIONS.to_vec(),
        };
        provenance.validate()?;
        Ok(provenance)
    }
    pub fn source_service(acquisition: Acquisition) -> Self {
        Self {
            acquisition,
            adapter_version: "source-service-v1".into(),
            source_format: "proteusSourceServiceRecord".into(),
            original_object_id: None,
            original_byte_length: None,
            hash_algorithm: HashAlgorithm::Sha256,
            limitations: ALL_LIMITATIONS.to_vec(),
        }
    }
}
fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
fn required_byte_length<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Number>::deserialize(deserializer)?;
    value
        .map(|value| {
            if let Some(integer) = value.as_u64() {
                return Ok(integer);
            }
            let number = value
                .as_f64()
                .ok_or_else(|| serde::de::Error::custom(INVALID))?;
            if number.is_finite()
                && number >= 0.0
                && number <= MAX_SAFE_INTEGER as f64
                && number.fract() == 0.0
            {
                Ok(number as u64)
            } else {
                Err(serde::de::Error::custom(INVALID))
            }
        })
        .transpose()
}
#[cfg(test)]
#[path = "source-provenance-tests.rs"]
mod tests;
