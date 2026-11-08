//! Padded token work counts toward CPU/RAM budgets; token 0 is a valid CLS.
use super::{EmbeddingBudget, EmbeddingError};
pub(super) struct TokenBatch {
    pub ids: Vec<u32>,
    pub masks: Vec<u32>,
    pub rows: usize,
    pub width: usize,
}
impl TokenBatch {
    pub fn new(sequences: &[Vec<u32>], budget: &EmbeddingBudget) -> Result<Self, EmbeddingError> {
        let width = sequences.iter().map(Vec::len).max().unwrap_or(0);
        if sequences.is_empty()
            || sequences.len() > budget.max_batch_items
            || width == 0
            || width > 512
            || sequences
                .iter()
                .any(|s| s.is_empty() || s.iter().any(|id| *id >= 250037))
        {
            return Err(EmbeddingError::InvalidInput);
        }
        let tokens = sequences
            .len()
            .checked_mul(width)
            .ok_or(EmbeddingError::BatchBudget)?;
        if tokens > budget.max_batch_tokens {
            return Err(EmbeddingError::BatchBudget);
        }
        let mut ids = vec![1; tokens];
        let mut masks = vec![0; tokens];
        for (row, sequence) in sequences.iter().enumerate() {
            ids[row * width..row * width + sequence.len()].copy_from_slice(sequence);
            masks[row * width..row * width + sequence.len()].fill(1);
        }
        Ok(Self {
            ids,
            masks,
            rows: sequences.len(),
            width,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn budget(tokens: usize) -> EmbeddingBudget {
        EmbeddingBudget {
            max_batch_tokens: tokens,
            max_batch_items: 8,
            max_resident_bytes: super::super::RESIDENT_RESERVATION_BYTES,
            max_duration_ms: 60000,
        }
    }
    #[test]
    fn cls_zero_is_attended_and_padding_counts_toward_the_budget() {
        let input = [vec![0, 41, 12, 20, 2], vec![0, 2]];
        assert!(matches!(
            TokenBatch::new(&input, &budget(7)),
            Err(EmbeddingError::BatchBudget)
        ));
        let batch = TokenBatch::new(&input, &budget(10)).unwrap();
        assert_eq!(batch.ids, [0, 41, 12, 20, 2, 0, 2, 1, 1, 1]);
        assert_eq!(batch.masks, [1, 1, 1, 1, 1, 1, 1, 0, 0, 0]);
    }
}
