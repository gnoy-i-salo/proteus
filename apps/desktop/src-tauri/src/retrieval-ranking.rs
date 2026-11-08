//! Bounded deterministic reciprocal-rank fusion for hybrid retrieval.
//!
//! Fuses two INDEPENDENT, already authorized ranked candidate lists — lexical
//! and semantic — into one deterministic ranking with per-document diversity.
//! This helper does not authorize access, filter grants or holdout, expand
//! source originals, or touch source text: the native host filters both lists
//! first and re-authorizes again before returning or using any text.
//! Semantic candidates are never restricted to lexical hits, and an honest
//! empty input list simply contributes no candidates.
//!
//! Ranks are 1-based positions in each input list; the fused score is the
//! reciprocal-rank sum `1/(60+rank)` over the lists where a chunk appears.
//! Results are sorted by score descending, then `chunk_id` ascending, and the
//! ranked union is scanned with a per-document cap until `max_results`
//! entries are collected. Memory stays proportional to the bounded union of
//! the two lists; no source text is ever held here.

use std::collections::HashMap;

const RRF_K: f64 = 60.0;
const MAX_CANDIDATES_PER_LIST: usize = 1000;
const MAX_RESULTS: usize = 100;
const MAX_PER_DOCUMENT: usize = 100;
const MAX_ID_BYTES: usize = 256;

/// A pre-authorized ranked candidate from one retrieval list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankedReference {
    pub chunk_id: String,
    pub document_id: String,
}

/// A fused candidate with its 1-based ranks per list and RRF score.
#[derive(Clone, Debug, PartialEq)]
pub struct FusedReference {
    pub chunk_id: String,
    pub document_id: String,
    pub lexical_rank: Option<usize>,
    pub semantic_rank: Option<usize>,
    pub rrf_score: f64,
}

/// Bounded fusion budget. All caps are validated before any allocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FusionBudget {
    pub max_candidates_per_list: usize,
    pub max_results: usize,
    pub max_per_document: usize,
}

struct UnionEntry {
    document_id: String,
    lexical_rank: Option<usize>,
    semantic_rank: Option<usize>,
    rrf_score: f64,
}

fn validate_budget(budget: &FusionBudget) -> Result<(), String> {
    if budget.max_candidates_per_list == 0
        || budget.max_candidates_per_list > MAX_CANDIDATES_PER_LIST
    {
        return Err(format!(
            "max_candidates_per_list должен быть от 1 до {MAX_CANDIDATES_PER_LIST}."
        ));
    }
    if budget.max_results == 0 || budget.max_results > MAX_RESULTS {
        return Err(format!("max_results должен быть от 1 до {MAX_RESULTS}."));
    }
    if budget.max_per_document == 0 || budget.max_per_document > MAX_PER_DOCUMENT {
        return Err(format!(
            "max_per_document должен быть от 1 до {MAX_PER_DOCUMENT}."
        ));
    }
    Ok(())
}

fn validate_id(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_ID_BYTES || value.trim().is_empty() {
        return Err(format!(
            "{label} пуст, состоит только из пробелов или превышает {MAX_ID_BYTES} байт UTF-8."
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label} содержит управляющие символы."));
    }
    Ok(())
}

fn list_size_error(label: &str) -> String {
    format!("{label} превышает допустимое число кандидатов.")
}

pub fn fuse(
    lexical: &[RankedReference],
    semantic: &[RankedReference],
    budget: &FusionBudget,
    checkpoint: &impl Fn() -> Result<(), String>,
) -> Result<Vec<FusedReference>, String> {
    checkpoint()?;
    validate_budget(budget)?;
    if lexical.len() > budget.max_candidates_per_list {
        return Err(list_size_error("Лексический список"));
    }
    if semantic.len() > budget.max_candidates_per_list {
        return Err(list_size_error("Семантический список"));
    }

    let mut union: HashMap<String, UnionEntry> = HashMap::new();
    for (index, candidate) in lexical.iter().enumerate() {
        checkpoint()?;
        let rank = index + 1;
        validate_id(&candidate.chunk_id, "Идентификатор чанка")?;
        validate_id(&candidate.document_id, "Идентификатор документа")?;
        if union.contains_key(&candidate.chunk_id) {
            return Err("Лексический список содержит повторяющийся chunk_id.".to_string());
        }
        union.insert(
            candidate.chunk_id.clone(),
            UnionEntry {
                document_id: candidate.document_id.clone(),
                lexical_rank: Some(rank),
                semantic_rank: None,
                rrf_score: 1.0 / (RRF_K + rank as f64),
            },
        );
    }
    for (index, candidate) in semantic.iter().enumerate() {
        checkpoint()?;
        let rank = index + 1;
        validate_id(&candidate.chunk_id, "Идентификатор чанка")?;
        validate_id(&candidate.document_id, "Идентификатор документа")?;
        match union.get_mut(&candidate.chunk_id) {
            Some(entry) => {
                if entry.document_id != candidate.document_id {
                    return Err(
                        "Один chunk_id указывает на разные документы в списках.".to_string()
                    );
                }
                if entry.semantic_rank.is_some() {
                    return Err("Семантический список содержит повторяющийся chunk_id.".to_string());
                }
                entry.semantic_rank = Some(rank);
                entry.rrf_score += 1.0 / (RRF_K + rank as f64);
            }
            None => {
                union.insert(
                    candidate.chunk_id.clone(),
                    UnionEntry {
                        document_id: candidate.document_id.clone(),
                        lexical_rank: None,
                        semantic_rank: Some(rank),
                        rrf_score: 1.0 / (RRF_K + rank as f64),
                    },
                );
            }
        }
    }

    let mut ranked: Vec<(&String, &UnionEntry)> = union.iter().collect();
    ranked.sort_by(|left, right| {
        right
            .1
            .rrf_score
            .total_cmp(&left.1.rrf_score)
            .then_with(|| left.0.cmp(right.0))
    });

    let mut per_document: HashMap<&str, usize> = HashMap::new();
    let mut results = Vec::new();
    for (chunk_id, entry) in &ranked {
        checkpoint()?;
        let seen = per_document.entry(entry.document_id.as_str()).or_insert(0);
        if *seen >= budget.max_per_document {
            continue;
        }
        *seen += 1;
        results.push(FusedReference {
            chunk_id: (*chunk_id).clone(),
            document_id: entry.document_id.clone(),
            lexical_rank: entry.lexical_rank,
            semantic_rank: entry.semantic_rank,
            rrf_score: entry.rrf_score,
        });
        if results.len() >= budget.max_results {
            break;
        }
    }
    checkpoint()?;
    Ok(results)
}

#[cfg(test)]
#[path = "retrieval-ranking-tests.rs"]
mod tests;
