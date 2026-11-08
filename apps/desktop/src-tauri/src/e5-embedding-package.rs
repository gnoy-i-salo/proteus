//! Checked local model bytes. This module never downloads or reads CLI state.
use super::{EmbeddingError, Guard, UnavailableReason};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};
pub const RUNTIME_VERSION: &str = "candle:0.11.1:cpuF32:maskedMeanF64L2:1";
#[derive(Clone, Debug)]
pub struct ModelFiles {
    pub weights: PathBuf,
    pub config: PathBuf,
    pub tokenizer: PathBuf,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FilePin {
    pub bytes: usize,
    pub sha256: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ModelPin {
    pub schema_version: u8,
    pub model_id: String,
    pub revision: String,
    pub weights: FilePin,
    pub config: FilePin,
    pub dimensions: usize,
    pub runtime_version: String,
}
pub(super) fn pin() -> Result<ModelPin, EmbeddingError> {
    let pin: ModelPin = serde_json::from_str(include_str!(
        "../../../../scripts/e5-embedding-package.json"
    ))
    .map_err(|_| EmbeddingError::Unavailable(UnavailableReason::UnsupportedRuntime))?;
    let hash = |s: &str| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if pin.schema_version != 1
        || pin.model_id != "intfloat/multilingual-e5-small"
        || pin.revision.len() != 40
        || !pin.revision.bytes().all(|b| b.is_ascii_hexdigit())
        || pin.dimensions != 384
        || pin.runtime_version != RUNTIME_VERSION
        || !(1..=512 * 1024 * 1024).contains(&pin.weights.bytes)
        || !(1..=65536).contains(&pin.config.bytes)
        || !hash(&pin.weights.sha256)
        || !hash(&pin.config.sha256)
    {
        return Err(EmbeddingError::Unavailable(
            UnavailableReason::UnsupportedRuntime,
        ));
    }
    Ok(pin)
}
pub(super) fn read(
    path: &Path,
    pin: &FilePin,
    guard: &Guard<'_>,
) -> Result<Vec<u8>, EmbeddingError> {
    guard.check()?;
    let mut file = std::fs::File::open(path).map_err(|error| {
        EmbeddingError::Unavailable(if error.kind() == ErrorKind::NotFound {
            UnavailableReason::MissingModel
        } else {
            UnavailableReason::InvalidChecksum
        })
    })?;
    let metadata = file
        .metadata()
        .map_err(|_| EmbeddingError::Unavailable(UnavailableReason::InvalidChecksum))?;
    if !metadata.is_file() || metadata.len() != pin.bytes as u64 {
        return Err(EmbeddingError::Unavailable(
            UnavailableReason::InvalidChecksum,
        ));
    }
    let mut bytes = Vec::with_capacity(pin.bytes);
    let mut hash = Sha256::new();
    let mut block = vec![0u8; 1024 * 1024];
    loop {
        guard.check()?;
        let size = file
            .read(&mut block)
            .map_err(|_| EmbeddingError::Unavailable(UnavailableReason::InvalidChecksum))?;
        if size == 0 {
            break;
        }
        if bytes.len().checked_add(size).is_none_or(|n| n > pin.bytes) {
            return Err(EmbeddingError::Unavailable(
                UnavailableReason::InvalidChecksum,
            ));
        }
        hash.update(&block[..size]);
        bytes.extend_from_slice(&block[..size]);
    }
    guard.check()?;
    if bytes.len() != pin.bytes || format!("{:x}", hash.finalize()) != pin.sha256 {
        return Err(EmbeddingError::Unavailable(
            UnavailableReason::InvalidChecksum,
        ));
    }
    Ok(bytes)
}
pub(super) fn fingerprint(pin: &ModelPin, tokenizer: &str) -> Result<String, EmbeddingError> {
    let bytes = serde_json::to_vec(&(pin, tokenizer))
        .map_err(|_| EmbeddingError::Unavailable(UnavailableReason::UnsupportedRuntime))?;
    Ok(format!("e5Embedding:1:{:x}", Sha256::digest(bytes)))
}
