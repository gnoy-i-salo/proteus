use super::*;
use std::cell::Cell;
fn limits() -> RecallLimits {
    RecallLimits {
        max_calls: 3,
        max_candidates: 10,
        max_episodes: 3,
        max_source_bytes: 100,
        max_context_tokens: 20,
        max_duration_ms: 100,
    }
}
fn charge() -> RetrievalCharge {
    RetrievalCharge {
        candidates: 3,
        episodes: 1,
        source_bytes: 20,
        context_tokens: 4,
    }
}
#[test]
fn successive_calls_receive_remaining_budget_and_charge_exact_observed_usage() {
    let mut ledger = RecallLedger::new(limits(), 1000).unwrap();
    assert_eq!(
        ledger
            .run(
                || 1010,
                |allowance| {
                    assert_eq!(allowance.remaining.candidates, 10);
                    assert_eq!(allowance.duration_ms, 90);
                    Ok(("first", charge()))
                }
            )
            .unwrap(),
        "first"
    );
    ledger
        .run(
            || 1020,
            |allowance| {
                assert_eq!(
                    allowance.remaining,
                    RetrievalCharge {
                        candidates: 7,
                        episodes: 2,
                        source_bytes: 80,
                        context_tokens: 16
                    }
                );
                assert_eq!(allowance.duration_ms, 80);
                Ok(((), charge()))
            },
        )
        .unwrap();
    assert_eq!(
        ledger.usage(),
        (
            2,
            RetrievalCharge {
                candidates: 6,
                episodes: 2,
                source_bytes: 40,
                context_tokens: 8
            }
        )
    );
}
#[test]
fn failed_attempts_consume_calls_but_do_not_claim_disclosed_source_bytes() {
    let mut ledger = RecallLedger::new(limits(), 1000).unwrap();
    for _ in 0..3 {
        assert_eq!(
            ledger
                .run::<()>(|| 1001, |_| Err("cancel exact  ".into()))
                .unwrap_err(),
            "cancel exact  "
        );
    }
    assert_eq!(ledger.usage(), (3, RetrievalCharge::default()));
    assert!(ledger
        .run::<()>(|| 1002, |_| panic!("call limit bypassed"))
        .is_err());
}
#[test]
fn every_resource_limit_prevents_later_calls_before_work_starts() {
    for field in 0..4 {
        let mut ledger = RecallLedger::new(limits(), 1000).unwrap();
        let mut used = RetrievalCharge::default();
        match field {
            0 => used.candidates = 10,
            1 => used.episodes = 3,
            2 => used.source_bytes = 100,
            _ => used.context_tokens = 20,
        }
        ledger.run(|| 1001, |_| Ok(((), used))).unwrap();
        assert!(ledger
            .run::<()>(|| 1002, |_| panic!("exhausted resource bypassed"))
            .is_err());
        assert_eq!(ledger.usage(), (1, used));
    }
}
#[test]
fn oversized_host_charges_fail_closed_without_returning_results_or_reopening_budget() {
    for field in 0..4 {
        let mut ledger = RecallLedger::new(limits(), 1000).unwrap();
        let mut used = charge();
        match field {
            0 => used.candidates = usize::MAX,
            1 => used.episodes = usize::MAX,
            2 => used.source_bytes = usize::MAX,
            _ => used.context_tokens = usize::MAX,
        }
        assert!(ledger
            .run(|| 1001, |_| Ok(("must not escape", used)))
            .is_err());
        assert!(ledger
            .run::<()>(|| 1002, |_| panic!("invalid charge reopened ledger"))
            .is_err());
    }
}
#[test]
fn deadline_and_clock_rollback_discard_even_finished_results() {
    for end in [999, 1100] {
        let mut ledger = RecallLedger::new(limits(), 1000).unwrap();
        let clock = Cell::new(1001);
        assert!(ledger
            .run(
                || clock.get(),
                |_| {
                    clock.set(end);
                    Ok(("late", charge()))
                }
            )
            .is_err());
        assert!(ledger
            .run::<()>(|| 1002, |_| panic!("invalid time reopened ledger"))
            .is_err());
    }
    let mut ledger = RecallLedger::new(limits(), 1000).unwrap();
    assert!(ledger
        .run::<()>(|| 1100, |_| panic!("expired ledger ran work"))
        .is_err());
}
#[test]
fn invalid_limits_and_clock_overflow_are_rejected_at_creation() {
    for field in 0..6 {
        let mut value = limits();
        match field {
            0 => value.max_calls = 0,
            1 => value.max_candidates = usize::MAX,
            2 => value.max_episodes = 0,
            3 => value.max_source_bytes = usize::MAX,
            4 => value.max_context_tokens = 0,
            _ => value.max_duration_ms = 0,
        }
        assert!(RecallLedger::new(value, 1000).is_err());
    }
    assert!(RecallLedger::new(limits(), -1).is_err());
    assert!(RecallLedger::new(limits(), i64::MAX).is_err());
}

#[test]
fn clock_cannot_move_back_between_calls_and_expiry_keeps_the_original_failure() {
    let mut ledger = RecallLedger::new(limits(), 1000).unwrap();
    ledger.run(|| 1050, |_| Ok(((), charge()))).unwrap();
    assert!(ledger
        .run::<()>(|| 1049, |_| panic!("clock rollback ran work"))
        .is_err());
    let mut ledger = RecallLedger::new(limits(), 1000).unwrap();
    let clock = Cell::new(1001);
    assert_eq!(
        ledger
            .run::<()>(
                || clock.get(),
                |_| {
                    clock.set(1200);
                    Err("revoked exact  ".into())
                }
            )
            .unwrap_err(),
        "revoked exact  "
    );
    assert!(ledger
        .run::<()>(|| 1002, |_| panic!("expired error reopened ledger"))
        .is_err());
}

#[test]
fn checked_clock_can_revalidate_authority_before_and_after_the_action() {
    let mut ledger = RecallLedger::new(limits(), 1000).unwrap();
    assert_eq!(
        ledger
            .run_checked(|| Ok(1001), |_| Ok(("allowed", charge())))
            .unwrap(),
        "allowed"
    );
    assert_eq!(ledger.usage(), (1, charge()));
}
#[test]
fn a_failed_preflight_guard_never_runs_work_or_reopens_the_session() {
    let mut ledger = RecallLedger::new(limits(), 1000).unwrap();
    assert_eq!(
        ledger
            .run_checked::<()>(
                || Err("revoked before  ".into()),
                |_| panic!("denied work ran")
            )
            .unwrap_err(),
        "revoked before  "
    );
    assert_eq!(ledger.usage().0, 0);
    assert!(ledger
        .run_checked::<()>(|| Ok(1001), |_| panic!("revoked session reopened"))
        .is_err());
}
#[test]
fn a_failed_final_guard_discards_completed_results_and_preserves_reason() {
    let mut ledger = RecallLedger::new(limits(), 1000).unwrap();
    let completed = Cell::new(false);
    let result = ledger.run_checked(
        || {
            if completed.get() {
                Err("revoked after  ".into())
            } else {
                Ok(1001)
            }
        },
        |_| {
            completed.set(true);
            Ok(("private result", charge()))
        },
    );
    assert_eq!(result.unwrap_err(), "revoked after  ");
    assert_eq!(ledger.usage(), (1, RetrievalCharge::default()));
    assert!(ledger
        .run_checked::<()>(|| Ok(1002), |_| panic!("late revocation reopened session"))
        .is_err());
}
