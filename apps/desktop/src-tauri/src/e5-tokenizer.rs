//! Local, checksum-pinned E5 tokenization. No model download or inference here.
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
use tokenizers::Tokenizer;
pub const MODEL_REVISION: &str = "614241f622f53c4eeff9890bdc4f31cfecc418b3";
pub const TOKENIZER_SHA256: &str =
    "0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39";
const TOKENIZER_BYTES: usize = 17_082_730;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_SEQUENCE_TOKENS: usize = 512;
const INVALID: &str = "Локальный токенизатор E5 отсутствует, повреждён или имеет другую версию.";
#[derive(Clone, Copy)]
pub enum InputKind {
    Query,
    Passage,
}
impl InputKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Query => "query: ",
            Self::Passage => "passage: ",
        }
    }
}
pub struct E5Tokenizer {
    tokenizer: Tokenizer,
    fingerprint: String,
}
impl E5Tokenizer {
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|_| INVALID)?;
        if !file.metadata().map_err(|_| INVALID)?.is_file() {
            return Err(INVALID.into());
        }
        let mut bytes = Vec::new();
        file.take((TOKENIZER_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| INVALID)?;
        Self::verified(&bytes, TOKENIZER_BYTES, TOKENIZER_SHA256)
    }
    fn verified(bytes: &[u8], expected_size: usize, expected_hash: &str) -> Result<Self, String> {
        if bytes.len() != expected_size || format!("{:x}", Sha256::digest(bytes)) != expected_hash {
            return Err(INVALID.into());
        }
        let mut tokenizer = Tokenizer::from_bytes(bytes).map_err(|_| INVALID)?;
        // The full count determines chunk boundaries. Stored truncation/padding
        // must never disguise an over-budget input or invent source tokens.
        tokenizer.with_truncation(None).map_err(|_| INVALID)?;
        tokenizer.with_padding(None);
        Ok(Self { tokenizer, fingerprint:format!("e5Tokenizer:1:{MODEL_REVISION}:{expected_hash}:tokenizers:0.23.2:queryPassage:1:specialTokens:1") })
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn count_passage(&self, text: &str) -> Result<usize, String> {
        self.encode(text, InputKind::Passage).map(|ids| ids.len())
    }
    pub fn model_input(&self, text: &str, kind: InputKind) -> Result<Vec<u32>, String> {
        let ids = self.encode(text, kind)?;
        if ids.len() > MAX_SEQUENCE_TOKENS {
            return Err("Текст превышает окно локальной модели; разделите его на чанки.".into());
        }
        Ok(ids)
    }
    fn encode(&self, text: &str, kind: InputKind) -> Result<Vec<u32>, String> {
        if text.len() > MAX_TEXT_BYTES {
            return Err("Текст превышает бюджет токенизации.".into());
        }
        self.tokenizer
            .encode(format!("{}{text}", kind.prefix()), true)
            .map(|encoding| encoding.get_ids().to_vec())
            .map_err(|_| "Не удалось токенизировать текст локально.".into())
    }
}
#[cfg(test)]
#[path = "e5-tokenizer-tests.rs"]
mod tests;
