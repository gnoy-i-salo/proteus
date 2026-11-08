//! Native embedding port for authorized scenarios. Test adapters stay in tests.
use super::{
    runtime::{E5Runtime, EmbeddingBudget, EmbeddingError},
    tokenizer::InputKind,
};
pub trait EmbeddingModel {
    fn fingerprint(&self) -> &str;
    fn tokenizer_fingerprint(&self) -> &str;
    fn dimensions(&self) -> usize;
    fn embed(
        &self,
        texts: &[&str],
        kind: InputKind,
        budget: &EmbeddingBudget,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError>;
}
impl EmbeddingModel for E5Runtime {
    fn fingerprint(&self) -> &str {
        self.fingerprint()
    }
    fn tokenizer_fingerprint(&self) -> &str {
        self.tokenizer_fingerprint()
    }
    fn dimensions(&self) -> usize {
        384
    }
    fn embed(
        &self,
        texts: &[&str],
        kind: InputKind,
        budget: &EmbeddingBudget,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        self.embed(texts, kind, self.fingerprint(), budget, check)
    }
}

pub fn model_error(error: EmbeddingError) -> String {
    match error {
        EmbeddingError::Interrupted(reason) => reason,
        EmbeddingError::Unavailable(_) => "Локальная модель недоступна; проверьте её файлы.".into(),
        EmbeddingError::TimeBudget => "Превышено время локальной обработки.".into(),
        _ => "Локальная модель отклонила данные, профиль или бюджет обработки.".into(),
    }
}
