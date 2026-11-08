//! Exact semantic ranking of pre-authorized vectors. This module grants no access.
#[derive(Clone, Copy)]
pub struct VectorCandidate<'a> {
    pub chunk_id: &'a str,
    pub document_id: &'a str,
    pub fingerprint: &'a str,
    pub vector: &'a [f32],
}
#[derive(Debug, PartialEq)]
pub struct SemanticHit {
    pub chunk_id: String,
    pub document_id: String,
    pub cosine: f64,
}
#[derive(Clone, Copy)]
pub struct SearchBudget {
    pub max_scanned: usize,
    pub max_results: usize,
}
// Candidates must already be restricted to the host's current grants/reference
// partition. Search does not acquire or authorize them; the host rechecks scope
// after ranking. At most 100000 unique IDs and 1000 hits are retained, never the
// whole vector collection. The iterator owner controls input/storage memory.
use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashSet},
};
struct Ranked(SemanticHit);
impl PartialEq for Ranked {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Ranked {}
impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .0
            .cosine
            .total_cmp(&self.0.cosine)
            .then_with(|| self.0.chunk_id.cmp(&other.0.chunk_id))
    }
}
fn valid_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
fn norm(vector: &[f32]) -> Result<f64, String> {
    if vector.is_empty() || vector.len() > 4096 || vector.iter().any(|n| !n.is_finite()) {
        return Err("Недопустимый вектор поиска.".into());
    }
    let norm = vector
        .iter()
        .map(|&n| f64::from(n).powi(2))
        .sum::<f64>()
        .sqrt();
    if norm == 0. || !norm.is_finite() {
        return Err("Недопустимая норма вектора поиска.".into());
    }
    Ok(norm)
}
pub fn search<'a>(
    query: &[f32],
    fingerprint: &str,
    candidates: impl IntoIterator<Item = VectorCandidate<'a>>,
    budget: SearchBudget,
    checkpoint: &impl Fn() -> Result<(), String>,
) -> Result<Vec<SemanticHit>, String> {
    search_stream(query, fingerprint, budget, checkpoint, |accept| {
        for row in candidates {
            accept(row)?;
        }
        Ok(())
    })
}

/// The scanner lends one row at a time and propagates storage errors. Only
/// IDs and bounded top hits are retained; row/vector buffers may be reused.
/// The caller applies current scope before supplying candidates.
pub fn search_stream(
    query: &[f32],
    fingerprint: &str,
    budget: SearchBudget,
    checkpoint: &impl Fn() -> Result<(), String>,
    scan: impl FnOnce(&mut dyn FnMut(VectorCandidate<'_>) -> Result<(), String>) -> Result<(), String>,
) -> Result<Vec<SemanticHit>, String> {
    if !(1..=100000).contains(&budget.max_scanned)
        || !(1..=1000).contains(&budget.max_results)
        || !valid_id(fingerprint)
    {
        return Err("Недопустимый бюджет или профиль векторного поиска.".into());
    }
    checkpoint()?;
    let query_norm = norm(query)?;
    let mut seen = HashSet::new();
    let mut top = BinaryHeap::<Ranked>::new();
    let mut position = 0;
    let mut consume = |row: VectorCandidate<'_>| -> Result<(), String> {
        checkpoint()?;
        if position >= budget.max_scanned {
            return Err("Превышен бюджет кандидатов векторного поиска.".into());
        }
        position += 1;
        if row.fingerprint != fingerprint
            || row.vector.len() != query.len()
            || !valid_id(row.chunk_id)
            || !valid_id(row.document_id)
        {
            return Err("Вектор не соответствует профилю поиска.".into());
        }
        if !seen.insert(row.chunk_id.to_owned()) {
            return Err("Повторный идентификатор векторного кандидата.".into());
        }
        let denominator = query_norm * norm(row.vector)?;
        let cosine = query
            .iter()
            .zip(row.vector)
            .map(|(&a, &b)| f64::from(a) * f64::from(b))
            .sum::<f64>()
            / denominator;
        // Finite f32 inputs accumulated as f64; clamp only round-off beyond ±1.
        let ranked = Ranked(SemanticHit {
            chunk_id: row.chunk_id.into(),
            document_id: row.document_id.into(),
            cosine: cosine.clamp(-1., 1.),
        });
        if top.len() < budget.max_results {
            top.push(ranked);
        } else if let Some(mut worst) = top.peek_mut() {
            if ranked < *worst {
                *worst = ranked;
            }
        }
        Ok(())
    };
    let mut row_error: Option<String> = None;
    let scanned = scan(&mut |row| {
        if let Some(error) = &row_error {
            return Err(error.clone());
        }
        let result = consume(row);
        if let Err(error) = &result {
            row_error = Some(error.clone());
        }
        result
    });
    if let Some(error) = row_error {
        return Err(error);
    }
    scanned?;
    let hits = top
        .into_sorted_vec()
        .into_iter()
        .map(|ranked| ranked.0)
        .collect();
    checkpoint()?;
    Ok(hits)
}
#[cfg(test)]
#[path = "embedding-search-tests.rs"]
mod tests;
