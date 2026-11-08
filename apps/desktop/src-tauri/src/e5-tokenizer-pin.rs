//! One checked-in package pin is shared by the downloader, build and runtime.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
const INVALID: &str = "Локальный токенизатор E5 отсутствует, повреждён или имеет другую версию.";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenizerPin {
    pub revision: String,
    pub bytes: usize,
    pub sha256: String,
}
pub fn pin() -> Result<TokenizerPin, String> {
    serde_json::from_str(include_str!("../../../../scripts/e5-package.json"))
        .map_err(|_| "Неверная спецификация локального токенизатора.".into())
}
pub fn read(path: &Path, pin: &TokenizerPin) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|_| INVALID)?;
    if !file.metadata().map_err(|_| INVALID)?.is_file() {
        return Err(INVALID.into());
    }
    let mut bytes = Vec::new();
    file.take((pin.bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| INVALID)?;
    if bytes.len() != pin.bytes || format!("{:x}", Sha256::digest(&bytes)) != pin.sha256 {
        return Err(INVALID.into());
    }
    Ok(bytes)
}
