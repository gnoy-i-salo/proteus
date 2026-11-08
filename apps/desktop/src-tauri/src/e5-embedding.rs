//! Local CPU embedding inference. No downloads, CLI calls or authorization here.
//! Native orchestration must authorize model processing before supplying text.
#[path = "e5-embedding-batch.rs"]
mod batch;
#[path = "e5-embedding-math.rs"]
mod math;
#[path = "e5-embedding-package.rs"]
mod package;
pub use package::{cache_descriptor, ModelFiles};
use std::time::{Duration, Instant};
pub const RESIDENT_RESERVATION_BYTES: u64 = 1536 * 1024 * 1024;
pub const MAX_BATCH_ITEMS: usize = 8;
pub const MAX_BATCH_TOKENS: usize = 4096;
#[derive(Clone, Copy, Debug)]
pub struct EmbeddingBudget {
    pub max_batch_tokens: usize,
    pub max_batch_items: usize,
    pub max_resident_bytes: u64,
    pub max_duration_ms: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnavailableReason {
    MissingModel,
    InvalidChecksum,
    UnsupportedRuntime,
}
#[derive(Debug, PartialEq, Eq)]
pub enum EmbeddingError {
    Unavailable(UnavailableReason),
    InvalidBudget,
    ResidentBudget,
    BatchBudget,
    TimeBudget,
    Interrupted(String),
    InvalidInput,
    ProfileMismatch,
    InferenceFailed,
}
impl EmbeddingBudget {
    fn validate(&self) -> Result<(), EmbeddingError> {
        if !(1..=MAX_BATCH_ITEMS).contains(&self.max_batch_items)
            || !(1..=MAX_BATCH_TOKENS).contains(&self.max_batch_tokens)
            || self.max_resident_bytes == 0
            || self.max_resident_bytes > 9_007_199_254_740_991
            || !(1..=3_600_000).contains(&self.max_duration_ms)
        {
            return Err(EmbeddingError::InvalidBudget);
        }
        if self.max_resident_bytes < RESIDENT_RESERVATION_BYTES {
            return Err(EmbeddingError::ResidentBudget);
        }
        Ok(())
    }
}
struct Guard<'a> {
    started: Instant,
    duration: Duration,
    external: &'a dyn Fn() -> Result<(), String>,
}
impl<'a> Guard<'a> {
    fn new(
        budget: &EmbeddingBudget,
        external: &'a dyn Fn() -> Result<(), String>,
    ) -> Result<Self, EmbeddingError> {
        budget.validate()?;
        let guard = Self {
            started: Instant::now(),
            duration: Duration::from_millis(budget.max_duration_ms),
            external,
        };
        guard.check()?;
        Ok(guard)
    }
    fn check(&self) -> Result<(), EmbeddingError> {
        (self.external)().map_err(EmbeddingError::Interrupted)?;
        if self.started.elapsed() >= self.duration {
            return Err(EmbeddingError::TimeBudget);
        }
        Ok(())
    }
}
pub struct E5Runtime {
    fingerprint: String,
    model: candle_transformers::models::bert::BertModel,
    tokenizer: super::tokenizer::E5Tokenizer,
}
impl E5Runtime {
    pub fn load(
        files: &ModelFiles,
        budget: &EmbeddingBudget,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<Self, EmbeddingError> {
        use candle_core::{DType, Device};
        use candle_transformers::models::bert::{BertModel, Config};
        let guard = Guard::new(budget, check)?;
        let pin = package::pin()?;
        let unsupported = || EmbeddingError::Unavailable(UnavailableReason::UnsupportedRuntime);
        let tokenizer_pin = super::tokenizer::package::pin().map_err(|_| unsupported())?;
        if tokenizer_pin.revision != pin.revision {
            return Err(unsupported());
        }
        let config = package::read(&files.config, &pin.config, &guard)?;
        let config: Config = serde_json::from_slice(&config).map_err(|_| unsupported())?;
        if config.hidden_size != pin.dimensions
            || config.num_hidden_layers != 12
            || config.num_attention_heads != 12
            || config.max_position_embeddings != 512
            || config.vocab_size != 250037
        {
            return Err(unsupported());
        }
        let bytes = package::read(&files.weights, &pin.weights, &guard)?;
        let builder =
            candle_nn::VarBuilder::from_buffered_safetensors(bytes, DType::F32, &Device::Cpu)
                .map_err(|_| unsupported())?;
        guard.check()?;
        let model = BertModel::load(builder, &config).map_err(|_| unsupported())?;
        guard.check()?;
        let tokenizer = super::tokenizer::E5Tokenizer::load(&files.tokenizer)
            .map_err(|_| EmbeddingError::Unavailable(UnavailableReason::InvalidChecksum))?;
        guard.check()?;
        let fingerprint = package::fingerprint(&pin, tokenizer.fingerprint())?;
        Ok(Self {
            fingerprint,
            model,
            tokenizer,
        })
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn embed(
        &self,
        texts: &[&str],
        kind: super::tokenizer::InputKind,
        expected: &str,
        budget: &EmbeddingBudget,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        use candle_core::{Device, Tensor};
        let guard = Guard::new(budget, check)?;
        if expected != self.fingerprint {
            return Err(EmbeddingError::ProfileMismatch);
        }
        if texts.is_empty() || texts.len() > budget.max_batch_items {
            return Err(EmbeddingError::BatchBudget);
        }
        let sequences = texts
            .iter()
            .map(|text| {
                guard.check()?;
                self.tokenizer
                    .model_input(text, kind)
                    .map_err(|_| EmbeddingError::InvalidInput)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let batch = batch::TokenBatch::new(&sequences, budget)?;
        let failed = |_| EmbeddingError::InferenceFailed;
        let ids =
            Tensor::from_vec(batch.ids, (batch.rows, batch.width), &Device::Cpu).map_err(failed)?;
        let attention =
            Tensor::from_vec(batch.masks.clone(), (batch.rows, batch.width), &Device::Cpu)
                .map_err(failed)?;
        guard.check()?;
        // Cancellation is cooperative at inference boundaries; no partial result escapes.
        let output = self
            .model
            .forward(&ids, &ids.zeros_like().map_err(failed)?, Some(&attention))
            .map_err(failed)?;
        guard.check()?;
        if output.dims3().map_err(failed)? != (batch.rows, batch.width, 384) {
            return Err(EmbeddingError::InferenceFailed);
        }
        let hidden = output.to_vec3::<f32>().map_err(failed)?;
        let vectors = hidden
            .iter()
            .zip(batch.masks.chunks_exact(batch.width))
            .map(|(row, mask)| {
                guard.check()?;
                math::pooled_vector(row, mask, 384).map_err(|_| EmbeddingError::InferenceFailed)
            })
            .collect::<Result<Vec<_>, _>>()?;
        guard.check()?;
        Ok(vectors)
    }
}
#[cfg(test)]
#[path = "e5-embedding-tests.rs"]
mod integration_tests;
#[cfg(test)]
mod tests {
    use super::*;
    fn budget() -> EmbeddingBudget {
        EmbeddingBudget {
            max_batch_tokens: 4096,
            max_batch_items: 8,
            max_resident_bytes: RESIDENT_RESERVATION_BYTES,
            max_duration_ms: 60000,
        }
    }
    #[test]
    fn invalid_and_under_reserved_budgets_fail_before_model_io() {
        for invalid in [
            EmbeddingBudget {
                max_batch_items: 0,
                ..budget()
            },
            EmbeddingBudget {
                max_batch_items: 9,
                ..budget()
            },
            EmbeddingBudget {
                max_batch_tokens: 0,
                ..budget()
            },
            EmbeddingBudget {
                max_batch_tokens: 4097,
                ..budget()
            },
            EmbeddingBudget {
                max_resident_bytes: 0,
                ..budget()
            },
            EmbeddingBudget {
                max_resident_bytes: 9_007_199_254_740_992,
                ..budget()
            },
            EmbeddingBudget {
                max_duration_ms: 0,
                ..budget()
            },
            EmbeddingBudget {
                max_duration_ms: 3_600_001,
                ..budget()
            },
        ] {
            assert_eq!(
                invalid.validate(),
                Err(EmbeddingError::InvalidBudget),
                "{invalid:?}"
            );
        }
        assert_eq!(
            EmbeddingBudget {
                max_resident_bytes: RESIDENT_RESERVATION_BYTES - 1,
                ..budget()
            }
            .validate(),
            Err(EmbeddingError::ResidentBudget)
        );
        assert_eq!(budget().validate(), Ok(()));
    }
    #[test]
    fn cancellation_and_elapsed_time_are_distinct_and_never_a_vector() {
        assert_eq!(
            Guard::new(&budget(), &|| Err("revoked".into())).err(),
            Some(EmbeddingError::Interrupted("revoked".into()))
        );
        let guard = Guard {
            started: Instant::now() - Duration::from_millis(5),
            duration: Duration::from_millis(1),
            external: &|| Ok(()),
        };
        assert_eq!(guard.check(), Err(EmbeddingError::TimeBudget));
    }
}
