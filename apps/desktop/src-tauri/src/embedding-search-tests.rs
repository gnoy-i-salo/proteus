use super::*;
use std::cell::Cell;
const PROFILE: &str = "e5Embedding:1:pinned";
fn candidate<'a>(id: &'a str, vector: &'a [f32]) -> VectorCandidate<'a> {
    VectorCandidate {
        chunk_id: id,
        document_id: id,
        fingerprint: PROFILE,
        vector,
    }
}
fn budget() -> SearchBudget {
    SearchBudget {
        max_scanned: 100,
        max_results: 2,
    }
}
#[test]
fn exact_cosine_keeps_semantic_only_hits_without_a_lexical_candidate_gate() {
    let rows = [
        candidate("unrelated", &[0., 1.]),
        candidate("semantic-only", &[2., 0.]),
        candidate("opposite", &[-1., 0.]),
    ];
    let hits = search(&[1., 0.], PROFILE, rows, budget(), &|| Ok(())).unwrap();
    assert_eq!(
        hits.iter().map(|h| h.chunk_id.as_str()).collect::<Vec<_>>(),
        ["semantic-only", "unrelated"]
    );
    assert_eq!(hits[0].cosine, 1.);
    assert_eq!(hits[1].cosine, 0.);
}
#[test]
fn ties_are_stable_by_exact_id_and_results_are_bounded() {
    let rows = [
        candidate("я", &[1., 0.]),
        candidate("é", &[1., 0.]),
        candidate("a", &[1., 0.]),
    ];
    let a = search(&[1., 0.], PROFILE, rows, budget(), &|| Ok(())).unwrap();
    let b = search(
        &[1., 0.],
        PROFILE,
        rows.into_iter().rev(),
        budget(),
        &|| Ok(()),
    )
    .unwrap();
    assert_eq!(a, b);
    assert_eq!(
        a.iter().map(|h| h.chunk_id.as_str()).collect::<Vec<_>>(),
        ["a", "é"]
    );
    assert!(search(&[1., 0.], PROFILE, [], budget(), &|| Ok(()))
        .unwrap()
        .is_empty());
}
#[test]
fn bad_vectors_profiles_duplicates_and_budget_overflow_fail_instead_of_partial_hits() {
    for vector in [
        vec![],
        vec![0., 0.],
        vec![f32::NAN, 0.],
        vec![f32::INFINITY, 0.],
        vec![1.],
    ] {
        assert!(search(
            &[1., 0.],
            PROFILE,
            [candidate("bad", &vector)],
            budget(),
            &|| Ok(())
        )
        .is_err());
        if vector.len() != 1 {
            assert!(search(&vector, PROFILE, [], budget(), &|| Ok(())).is_err());
        }
    }
    let mut foreign = candidate("foreign", &[1., 0.]);
    foreign.fingerprint = "other";
    assert!(search(&[1., 0.], PROFILE, [foreign], budget(), &|| Ok(())).is_err());
    let row = candidate("same", &[1., 0.]);
    assert!(search(&[1., 0.], PROFILE, [row, row], budget(), &|| Ok(())).is_err());
    for invalid in [
        SearchBudget {
            max_scanned: 0,
            max_results: 1,
        },
        SearchBudget {
            max_scanned: 100001,
            max_results: 1,
        },
        SearchBudget {
            max_scanned: 1,
            max_results: 0,
        },
        SearchBudget {
            max_scanned: 1,
            max_results: 1001,
        },
    ] {
        assert!(search(&[1., 0.], PROFILE, [], invalid, &|| Ok(())).is_err());
    }
    assert!(search(
        &[1., 0.],
        PROFILE,
        [row, candidate("second", &[0., 1.])],
        SearchBudget {
            max_scanned: 1,
            max_results: 1
        },
        &|| Ok(())
    )
    .is_err());
}
#[test]
fn cancellation_interrupts_streaming_and_discards_previously_ranked_hits() {
    let calls = Cell::new(0);
    let check = || {
        calls.set(calls.get() + 1);
        if calls.get() >= 3 {
            Err("revoked".into())
        } else {
            Ok(())
        }
    };
    assert_eq!(
        search(
            &[1., 0.],
            PROFILE,
            [
                candidate("first", &[1., 0.]),
                candidate("second", &[0., 1.])
            ],
            budget(),
            &check
        ),
        Err("revoked".into())
    );
    assert_eq!(
        search(
            &[1., 0.],
            PROFILE,
            [],
            budget(),
            &|| Err("cancelled".into())
        ),
        Err("cancelled".into())
    );
}
#[test]
fn invalid_identity_or_oversized_dimensions_never_enter_ranking() {
    for id in ["".into(), " \t".into(), "x\u{0}".into(), "я".repeat(129)] {
        assert!(search(
            &[1., 0.],
            PROFILE,
            [candidate(&id, &[1., 0.])],
            budget(),
            &|| Ok(())
        )
        .is_err());
    }
    assert!(search(&vec![1.; 4097], PROFILE, [], budget(), &|| Ok(())).is_err());
    assert!(search(&[1., 0.], "", [], budget(), &|| Ok(())).is_err());
}
#[test]
fn bounded_heap_selects_the_same_best_candidates_in_both_stream_orders() {
    let ids: Vec<_> = (0..512).map(|n| format!("chunk-{n:03}")).collect();
    let vectors: Vec<_> = (0..512).map(|n| [n as f32, 1.]).collect();
    let rows: Vec<_> = ids
        .iter()
        .zip(&vectors)
        .map(|(id, v)| candidate(id, v))
        .collect();
    let budget = SearchBudget {
        max_scanned: 512,
        max_results: 17,
    };
    let hits = search(&[1., 0.], PROFILE, rows.iter().copied(), budget, &|| Ok(())).unwrap();
    let reversed = search(&[1., 0.], PROFILE, rows.into_iter().rev(), budget, &|| {
        Ok(())
    })
    .unwrap();
    assert_eq!(hits, reversed);
    assert_eq!(
        hits.iter().map(|h| h.chunk_id.clone()).collect::<Vec<_>>(),
        (495..512)
            .rev()
            .map(|n| format!("chunk-{n:03}"))
            .collect::<Vec<_>>()
    );
}
#[test]
fn cancellation_after_exhaustion_and_invalid_low_ranked_rows_are_not_hidden() {
    let cancelled = Cell::new(false);
    let rows = std::iter::from_fn(|| {
        cancelled.set(true);
        None
    });
    assert_eq!(
        search(&[1., 0.], PROFILE, rows, budget(), &|| if cancelled.get() {
            Err("late revoke".into())
        } else {
            Ok(())
        }),
        Err("late revoke".into())
    );
    let rows = [
        candidate("best", &[1., 0.]),
        candidate("bad", &[f32::NAN, 0.]),
    ];
    assert!(search(
        &[1., 0.],
        PROFILE,
        rows,
        SearchBudget {
            max_scanned: 2,
            max_results: 1
        },
        &|| Ok(())
    )
    .is_err());
    let mut bad = candidate("valid", &[1., 0.]);
    bad.document_id = "\n";
    assert!(search(&[1., 0.], PROFILE, [bad], budget(), &|| Ok(())).is_err());
}
#[test]
fn finite_extreme_float32_values_use_float64_accumulation() {
    for value in [f32::MAX, f32::MIN_POSITIVE, f32::from_bits(1)] {
        let vector = [value, value];
        let hits = search(
            &vector,
            PROFILE,
            [candidate("same", &vector)],
            budget(),
            &|| Ok(()),
        )
        .unwrap();
        assert!((hits[0].cosine - 1.).abs() < 1e-12);
    }
}
