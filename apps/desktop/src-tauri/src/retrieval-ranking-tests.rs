//! Behavioral tests for bounded deterministic reciprocal-rank fusion.
//!
//! All fixtures are synthetic in-memory ranked lists. IDs include Cyrillic
//! and Unicode on purpose: output must preserve them byte-for-byte. This
//! helper performs no authorization and stores no source text, so nothing
//! here asserts grant, holdout or corpus behavior — the native host owns that.

use super::*;
use std::cell::Cell;

fn ok_checkpoint() -> impl Fn() -> Result<(), String> {
    || Ok(())
}

fn reference(chunk_id: &str, document_id: &str) -> RankedReference {
    RankedReference {
        chunk_id: chunk_id.to_string(),
        document_id: document_id.to_string(),
    }
}

fn budget(
    max_candidates_per_list: usize,
    max_results: usize,
    max_per_document: usize,
) -> FusionBudget {
    FusionBudget {
        max_candidates_per_list,
        max_results,
        max_per_document,
    }
}

fn counting_checkpoint<'a>(
    calls: &'a Cell<usize>,
    fail_at: Option<usize>,
    sentinel: &'a str,
) -> impl Fn() -> Result<(), String> + 'a {
    move || {
        let next = calls.get() + 1;
        calls.set(next);
        if fail_at == Some(next) {
            Err(sentinel.to_string())
        } else {
            Ok(())
        }
    }
}

#[test]
fn semantic_only_candidate_survives_empty_lexical_list() {
    let lexical = Vec::new();
    let semantic = vec![reference("s1", "doc-semantic")];

    let fused = fuse(&lexical, &semantic, &budget(10, 10, 10), &ok_checkpoint()).unwrap();

    assert_eq!(fused.len(), 1);
    assert_eq!(fused[0].chunk_id, "s1");
    assert_eq!(fused[0].document_id, "doc-semantic");
    assert_eq!(fused[0].lexical_rank, None);
    assert_eq!(fused[0].semantic_rank, Some(1));
    assert_eq!(fused[0].rrf_score, 1.0 / 61.0);
}

#[test]
fn lexical_only_candidates_keep_semantic_rank_none() {
    let lexical = vec![reference("l1", "doc-a"), reference("l2", "doc-b")];
    let semantic = Vec::new();

    let fused = fuse(&lexical, &semantic, &budget(10, 10, 10), &ok_checkpoint()).unwrap();

    assert_eq!(fused.len(), 2);
    assert_eq!(fused[0].chunk_id, "l1");
    assert_eq!(fused[0].lexical_rank, Some(1));
    assert_eq!(fused[0].semantic_rank, None);
    assert_eq!(fused[1].chunk_id, "l2");
    assert_eq!(fused[1].lexical_rank, Some(2));
}

#[test]
fn both_lists_empty_return_no_manufactured_results() {
    let fused = fuse(
        &Vec::new(),
        &Vec::new(),
        &budget(10, 10, 10),
        &ok_checkpoint(),
    )
    .unwrap();

    assert!(fused.is_empty());
}

#[test]
fn consensus_candidate_merges_once_and_outranks_single_list_hits() {
    let lexical = vec![reference("c1", "doc-1"), reference("c2", "doc-2")];
    let semantic = vec![reference("c3", "doc-3"), reference("c1", "doc-1")];

    let fused = fuse(&lexical, &semantic, &budget(10, 10, 10), &ok_checkpoint()).unwrap();

    assert_eq!(fused.len(), 3);
    assert_eq!(fused[0].chunk_id, "c1");
    assert_eq!(fused[0].lexical_rank, Some(1));
    assert_eq!(fused[0].semantic_rank, Some(2));
    assert_eq!(fused[0].rrf_score, 1.0 / 61.0 + 1.0 / 62.0);
    assert_eq!(fused[1].chunk_id, "c3");
    assert_eq!(fused[1].semantic_rank, Some(1));
    assert_eq!(fused[1].rrf_score, 1.0 / 61.0);
    assert_eq!(fused[2].chunk_id, "c2");
    assert_eq!(fused[2].rrf_score, 1.0 / 62.0);
    let merged = fused.iter().filter(|entry| entry.chunk_id == "c1").count();
    assert_eq!(merged, 1);
}

#[test]
fn same_leader_in_both_lists_merges_into_one_exact_score() {
    let lexical = vec![reference("к1", "док-а")];
    let semantic = vec![reference("к1", "док-а")];

    let fused = fuse(&lexical, &semantic, &budget(10, 10, 10), &ok_checkpoint()).unwrap();

    assert_eq!(fused.len(), 1);
    assert_eq!(fused[0].chunk_id, "к1");
    assert_eq!(fused[0].document_id, "док-а");
    assert_eq!(fused[0].lexical_rank, Some(1));
    assert_eq!(fused[0].semantic_rank, Some(1));
    assert_eq!(fused[0].rrf_score, 2.0 / 61.0);
}

#[test]
fn union_keeps_disjoint_candidates_from_both_lists() {
    let lexical = vec![reference("l1", "d1"), reference("l2", "d1")];
    let semantic = vec![reference("s1", "d2"), reference("s2", "d2")];

    let fused = fuse(&lexical, &semantic, &budget(10, 10, 10), &ok_checkpoint()).unwrap();

    let chunk_ids: Vec<&str> = fused.iter().map(|entry| entry.chunk_id.as_str()).collect();
    assert_eq!(chunk_ids, vec!["l1", "s1", "l2", "s2"]);
}

#[test]
fn ties_break_by_chunk_id_ascending_regardless_of_input_order() {
    let lexical = vec![reference("b", "doc"), reference("a", "doc")];
    let semantic = vec![reference("a", "doc"), reference("b", "doc")];

    let first = fuse(&lexical, &semantic, &budget(10, 10, 10), &ok_checkpoint()).unwrap();
    let again = fuse(&lexical, &semantic, &budget(10, 10, 10), &ok_checkpoint()).unwrap();

    let chunk_ids: Vec<&str> = first.iter().map(|entry| entry.chunk_id.as_str()).collect();
    assert_eq!(chunk_ids, vec!["a", "b"]);
    assert_eq!(first[0].rrf_score, first[1].rrf_score);
    assert_eq!(first, again);

    // A different input order assigns different ranks, but tied scores must
    // still surface in ascending chunk_id order.
    let reversed_lexical: Vec<RankedReference> = lexical.iter().rev().cloned().collect();
    let reversed_semantic: Vec<RankedReference> = semantic.iter().rev().cloned().collect();
    let second = fuse(
        &reversed_lexical,
        &reversed_semantic,
        &budget(10, 10, 10),
        &ok_checkpoint(),
    )
    .unwrap();

    let chunk_ids: Vec<&str> = second.iter().map(|entry| entry.chunk_id.as_str()).collect();
    assert_eq!(chunk_ids, vec!["a", "b"]);
    assert_eq!(second[0].rrf_score, second[1].rrf_score);
}

#[test]
fn diversity_cap_skips_capped_document_and_takes_next_eligible() {
    let lexical = vec![
        reference("c1", "doc-1"),
        reference("c2", "doc-1"),
        reference("c3", "doc-2"),
    ];
    let semantic = Vec::new();

    let fused = fuse(&lexical, &semantic, &budget(10, 10, 1), &ok_checkpoint()).unwrap();

    let chunk_ids: Vec<&str> = fused.iter().map(|entry| entry.chunk_id.as_str()).collect();
    assert_eq!(chunk_ids, vec!["c1", "c3"]);
}

#[test]
fn per_document_cap_two_allows_two_then_skips() {
    let lexical = vec![
        reference("c1", "doc-1"),
        reference("c2", "doc-1"),
        reference("c3", "doc-1"),
        reference("c4", "doc-2"),
    ];
    let semantic = Vec::new();

    let fused = fuse(&lexical, &semantic, &budget(10, 10, 2), &ok_checkpoint()).unwrap();

    let chunk_ids: Vec<&str> = fused.iter().map(|entry| entry.chunk_id.as_str()).collect();
    assert_eq!(chunk_ids, vec!["c1", "c2", "c4"]);
}

#[test]
fn max_results_stops_scanning_ranked_union() {
    let lexical = vec![
        reference("c1", "doc-1"),
        reference("c2", "doc-2"),
        reference("c3", "doc-3"),
    ];
    let semantic = Vec::new();

    let fused = fuse(&lexical, &semantic, &budget(10, 2, 10), &ok_checkpoint()).unwrap();

    let chunk_ids: Vec<&str> = fused.iter().map(|entry| entry.chunk_id.as_str()).collect();
    assert_eq!(chunk_ids, vec!["c1", "c2"]);
}

#[test]
fn budget_values_outside_defined_bounds_are_rejected() {
    let lexical = vec![reference("c1", "doc-1")];
    let semantic = vec![reference("c2", "doc-2")];

    assert!(fuse(&lexical, &semantic, &budget(0, 10, 10), &ok_checkpoint()).is_err());
    assert!(fuse(&lexical, &semantic, &budget(1001, 10, 10), &ok_checkpoint()).is_err());
    assert!(fuse(&lexical, &semantic, &budget(10, 0, 10), &ok_checkpoint()).is_err());
    assert!(fuse(&lexical, &semantic, &budget(10, 101, 10), &ok_checkpoint()).is_err());
    assert!(fuse(&lexical, &semantic, &budget(10, 10, 0), &ok_checkpoint()).is_err());
    assert!(fuse(&lexical, &semantic, &budget(10, 10, 101), &ok_checkpoint()).is_err());
}

#[test]
fn list_at_exact_budget_accepted_and_one_over_rejected() {
    let exact: Vec<RankedReference> = (0..1000)
        .map(|index| reference(&format!("chunk-{index}"), "doc"))
        .collect();
    let fused = fuse(
        &exact,
        &Vec::new(),
        &budget(1000, 100, 100),
        &ok_checkpoint(),
    )
    .unwrap();
    assert_eq!(fused.len(), 100);

    let overflow: Vec<RankedReference> = (0..1001)
        .map(|index| reference(&format!("chunk-{index}"), "doc"))
        .collect();
    assert!(fuse(
        &overflow,
        &Vec::new(),
        &budget(1000, 100, 100),
        &ok_checkpoint()
    )
    .is_err());
    assert!(fuse(
        &Vec::new(),
        &overflow,
        &budget(1000, 100, 100),
        &ok_checkpoint()
    )
    .is_err());
}

#[test]
fn duplicate_chunk_within_either_list_is_rejected() {
    let lexical_duplicate = vec![reference("c1", "doc-1"), reference("c1", "doc-2")];
    let semantic = vec![reference("c2", "doc-2")];
    assert!(fuse(
        &lexical_duplicate,
        &semantic,
        &budget(10, 10, 10),
        &ok_checkpoint()
    )
    .is_err());

    let lexical = vec![reference("c1", "doc-1")];
    let semantic_duplicate = vec![reference("c2", "doc-2"), reference("c2", "doc-2")];
    assert!(fuse(
        &lexical,
        &semantic_duplicate,
        &budget(10, 10, 10),
        &ok_checkpoint()
    )
    .is_err());
}

#[test]
fn same_chunk_with_conflicting_documents_between_lists_is_rejected() {
    let lexical = vec![reference("c1", "doc-1")];
    let semantic = vec![reference("c1", "doc-2")];

    assert!(fuse(&lexical, &semantic, &budget(10, 10, 10), &ok_checkpoint()).is_err());
}

#[test]
fn ids_validate_utf8_bounds_whitespace_and_control_characters() {
    let id_256 = "ч".repeat(128);
    assert_eq!(id_256.len(), 256);
    let id_257 = format!("{id_256}x");

    let lexical = vec![reference(&id_256, "doc-1")];
    let semantic = Vec::new();
    assert!(fuse(&lexical, &semantic, &budget(10, 10, 10), &ok_checkpoint()).is_ok());

    let lexical = vec![reference(&id_257, "doc-1")];
    assert!(fuse(&lexical, &semantic, &budget(10, 10, 10), &ok_checkpoint()).is_err());

    for invalid in ["", "   ", "\u{0007}", "a\nb", "a\u{0085}b"] {
        let lexical = vec![reference(invalid, "doc-1")];
        assert!(
            fuse(&lexical, &semantic, &budget(10, 10, 10), &ok_checkpoint()).is_err(),
            "chunk_id {invalid:?} must be rejected"
        );
        let lexical = vec![reference("c1", invalid)];
        assert!(
            fuse(&lexical, &semantic, &budget(10, 10, 10), &ok_checkpoint()).is_err(),
            "document_id {invalid:?} must be rejected"
        );
    }
}

#[test]
fn unicode_ids_are_preserved_byte_for_byte() {
    let lexical = vec![
        reference("Чанк Фонарь-Ω", "Док 7_β"),
        reference("plain", "d"),
    ];
    let semantic = vec![reference("Чанк Фонарь-Ω", "Док 7_β")];

    let fused = fuse(&lexical, &semantic, &budget(10, 10, 10), &ok_checkpoint()).unwrap();

    assert_eq!(fused[0].chunk_id, "Чанк Фонарь-Ω");
    assert_eq!(fused[0].document_id, "Док 7_β");
    assert_eq!(fused[0].lexical_rank, Some(1));
    assert_eq!(fused[0].semantic_rank, Some(1));
}

#[test]
fn cancellation_during_input_loop_propagates_exact_error() {
    let lexical = vec![reference("c1", "doc-1"), reference("c2", "doc-2")];
    let semantic = vec![reference("c3", "doc-3")];
    let calls = Cell::new(0usize);

    let error = fuse(
        &lexical,
        &semantic,
        &budget(10, 10, 10),
        &counting_checkpoint(&calls, Some(2), "cancelled-early"),
    )
    .unwrap_err();

    assert_eq!(error, "cancelled-early");
    assert_eq!(calls.get(), 2);
}

#[test]
fn cancellation_before_final_return_propagates_exact_error() {
    let lexical = vec![reference("c1", "doc-1")];
    let semantic = vec![reference("c2", "doc-2")];
    let calls = Cell::new(0usize);

    let error = fuse(
        &lexical,
        &semantic,
        &budget(10, 10, 10),
        &counting_checkpoint(&calls, Some(6), "cancelled-before-return"),
    )
    .unwrap_err();

    assert_eq!(error, "cancelled-before-return");
    assert_eq!(calls.get(), 6);
}

#[test]
fn every_checkpoint_failure_discards_all_results() {
    let lexical = vec![reference("c1", "doc-1")];
    let semantic = vec![reference("c2", "doc-2")];
    let calls = Cell::new(0);
    fuse(
        &lexical,
        &semantic,
        &budget(10, 10, 10),
        &counting_checkpoint(&calls, None, "unused"),
    )
    .unwrap();
    let checkpoints = calls.get();
    assert!(checkpoints > 0);
    for fail_at in 1..=checkpoints {
        calls.set(0);
        assert_eq!(
            fuse(
                &lexical,
                &semantic,
                &budget(10, 10, 10),
                &counting_checkpoint(&calls, Some(fail_at), "revoked")
            ),
            Err("revoked".into())
        );
    }
}

#[test]
fn borrowed_inputs_are_unchanged_and_caller_keeps_ownership() {
    let lexical = vec![reference("к1", "д1"), reference("k2", "d2")];
    let semantic = vec![reference("s1", "д1")];
    let lexical_before = lexical.clone();
    let semantic_before = semantic.clone();

    let fused = fuse(&lexical, &semantic, &budget(10, 10, 2), &ok_checkpoint()).unwrap();

    assert_eq!(lexical, lexical_before);
    assert_eq!(semantic, semantic_before);
    assert_eq!(lexical.len(), 2);
    assert_eq!(semantic.len(), 1);
    // "s1" sorts before "к1": the tie-break is deterministic UTF-8 byte order.
    let chunk_ids: Vec<&str> = fused.iter().map(|entry| entry.chunk_id.as_str()).collect();
    assert_eq!(chunk_ids, vec!["s1", "к1", "k2"]);
    assert_eq!(fused[1].lexical_rank, Some(1));
    assert_eq!(fused[1].semantic_rank, None);
    assert_eq!(fused[0].semantic_rank, Some(1));
}
