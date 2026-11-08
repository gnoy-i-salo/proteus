//! Exact framed input identity for a host-issued model-context lease. A matching
//! digest is not authorization; native dispatch separately consumes the lease.
use sha2::{Digest, Sha256};

const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;
#[derive(Debug, PartialEq, Eq)]
pub struct ModelInputIdentity {
    pub input_sha256: String,
    pub input_bytes: usize,
}

pub fn model_input_identity(system_prompt: &str, task: &str) -> Result<ModelInputIdentity, String> {
    if system_prompt.len().saturating_add(task.len()) > MAX_INPUT_BYTES {
        return Err("Вход модели превышает 4 МиБ.".into());
    }
    let bytes = serde_json::to_vec(&[system_prompt, task])
        .map_err(|_| "Не удалось определить точный вход модели.")?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err("Вход модели превышает 4 МиБ.".into());
    }
    Ok(ModelInputIdentity {
        input_sha256: format!("{:x}", Sha256::digest(&bytes)),
        input_bytes: bytes.len(),
    })
}

#[cfg(test)]
#[path = "memory-model-input-tests.rs"]
mod tests;
