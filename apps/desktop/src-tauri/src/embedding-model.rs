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
/// Keep the service's cancellation/deadline guard in every nested inference.
pub struct GuardedModel<'a> {
    pub inner: &'a dyn EmbeddingModel,
    pub guard: &'a dyn Fn() -> Result<(), String>,
}
impl EmbeddingModel for GuardedModel<'_> {
    fn fingerprint(&self) -> &str {
        self.inner.fingerprint()
    }
    fn tokenizer_fingerprint(&self) -> &str {
        self.inner.tokenizer_fingerprint()
    }
    fn dimensions(&self) -> usize {
        self.inner.dimensions()
    }
    fn embed(
        &self,
        texts: &[&str],
        kind: InputKind,
        budget: &EmbeddingBudget,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let guard = || {
            (self.guard)()?;
            check()
        };
        guard().map_err(EmbeddingError::Interrupted)?;
        let result = self.inner.embed(texts, kind, budget, &guard)?;
        guard().map_err(EmbeddingError::Interrupted)?;
        Ok(result)
    }
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
#[cfg(test)]
#[path = "embedding-model-tests.rs"]
mod tests;
