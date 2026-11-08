//! Masked mean pooling and deterministic f64-accumulated L2 normalization.
pub fn pooled_vector(
    hidden: &[Vec<f32>],
    mask: &[u32],
    dimensions: usize,
) -> Result<Vec<f32>, String> {
    if hidden.is_empty()
        || hidden.len() > 512
        || hidden.len() != mask.len()
        || dimensions == 0
        || dimensions > 384
        || mask.iter().any(|m| *m > 1)
        || hidden
            .iter()
            .any(|row| row.len() != dimensions || row.iter().any(|v| !v.is_finite()))
    {
        return Err("Invalid embedding tensor".into());
    }
    let count = mask.iter().filter(|m| **m == 1).count();
    if count == 0 {
        return Err("Empty attention mask".into());
    }
    let mut mean = vec![0f64; dimensions];
    for (row, active) in hidden.iter().zip(mask) {
        if *active == 1 {
            for (sum, value) in mean.iter_mut().zip(row) {
                *sum += f64::from(*value) / count as f64;
            }
        }
    }
    let norm = mean.iter().map(|v| v * v).sum::<f64>().sqrt();
    if !norm.is_finite() || norm == 0. {
        return Err("Invalid embedding norm".into());
    }
    Ok(mean
        .into_iter()
        .map(|value| (value / norm) as f32)
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn masked_mean_ignores_padding_and_l2_normalizes_the_actual_tokens() {
        let value = pooled_vector(
            &[vec![2., 0.], vec![0., 2.], vec![100., -100.]],
            &[1, 1, 0],
            2,
        )
        .unwrap();
        let expected = std::f32::consts::FRAC_1_SQRT_2;
        assert!((value[0] - expected).abs() < 1e-6);
        assert!((value[1] - expected).abs() < 1e-6);
        assert!((value.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 1e-6);
    }
    #[test]
    fn empty_zero_nonfinite_or_misshapen_vectors_never_become_fake_embeddings() {
        for (hidden, mask, dimensions) in [
            (vec![], vec![], 2),
            (vec![vec![1., 2.]], vec![0], 2),
            (vec![vec![0., 0.]], vec![1], 2),
            (vec![vec![f32::NAN, 1.]], vec![1], 2),
            (vec![vec![1., f32::INFINITY]], vec![1], 2),
            (vec![vec![1.]], vec![1], 2),
            (vec![vec![1., 2.]], vec![1, 1], 2),
            (vec![vec![1., 2.]], vec![2], 2),
        ] {
            assert!(pooled_vector(&hidden, &mask, dimensions).is_err());
        }
    }
}
