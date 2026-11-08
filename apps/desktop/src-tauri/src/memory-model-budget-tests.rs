//! Isolated std-only tests for the host-side cumulative model-call budget
//! ledger.
//!
//! Every clock is a fake `Cell`-backed script or a fixed failing closure; no
//! sleeps, no threads, no filesystem, no network, no real wall clock. The
//! ledger itself never performs model, CLI or IO work, so nothing here needs
//! more than single-threaded fake time.
use std::cell::Cell;

use super::*;

const START_MS: i64 = 100_000;
const DURATION_MS: u64 = 60_000;

/// `Cell`-backed fake wall clock; the test moves time by storing a new value.
fn clock_at(now: &Cell<i64>) -> impl Fn() -> Result<i64, String> + '_ {
    move || Ok(now.get())
}

fn new_ledger(max_calls: u32, max_input_bytes: u64) -> ModelCallLedger {
    match ModelCallLedger::new(max_calls, max_input_bytes, DURATION_MS, START_MS) {
        Ok(ledger) => ledger,
        Err(error) => panic!("in-range limits must construct a ledger, got {error:?}"),
    }
}

/// Constructor failures are compared through their error only: the ledger
/// itself deliberately implements neither `Debug` nor `PartialEq`.
fn assert_new_fails(result: Result<ModelCallLedger, BudgetError>, expected: BudgetError) {
    match result {
        Err(error) => assert_eq!(error, expected),
        Ok(_) => panic!("constructor must fail with {expected:?}"),
    }
}

#[test]
fn constructor_validates_limits_start_and_checked_deadline() {
    for max_calls in [0, MAX_LEDGER_CALLS + 1] {
        assert_new_fails(
            ModelCallLedger::new(max_calls, 1024, DURATION_MS, START_MS),
            BudgetError::InvalidLimits,
        );
    }
    for max_input_bytes in [0, MAX_LEDGER_INPUT_BYTES + 1] {
        assert_new_fails(
            ModelCallLedger::new(4, max_input_bytes, DURATION_MS, START_MS),
            BudgetError::InvalidLimits,
        );
    }
    for max_duration_ms in [0, MAX_LEDGER_DURATION_MS as u64 + 1] {
        assert_new_fails(
            ModelCallLedger::new(4, 1024, max_duration_ms, START_MS),
            BudgetError::InvalidLimits,
        );
    }
    assert_new_fails(
        ModelCallLedger::new(4, 1024, DURATION_MS, -1),
        BudgetError::InvalidStartTime,
    );
    // Checked deadline: a start too close to i64::MAX overflows the addition.
    assert_new_fails(
        ModelCallLedger::new(4, 1024, DURATION_MS, i64::MAX - 1000),
        BudgetError::InvalidLimits,
    );
    // Exact bounds are admitted.
    let ledger = ModelCallLedger::new(
        MAX_LEDGER_CALLS,
        MAX_LEDGER_INPUT_BYTES,
        MAX_LEDGER_DURATION_MS as u64,
        0,
    )
    .expect("boundary limits must construct");
    assert_eq!(
        ledger.usage(),
        BudgetUsage {
            used_calls: 0,
            max_calls: MAX_LEDGER_CALLS,
            used_input_bytes: 0,
            max_input_bytes: MAX_LEDGER_INPUT_BYTES,
        }
    );
    assert_eq!(ledger.closed_reason(), None);
}

#[test]
fn reserve_charges_calls_and_bytes_immediately_and_yields_receipt() {
    let mut ledger = new_ledger(10, 10_000);
    let now = Cell::new(START_MS);
    let receipt = ledger
        .reserve("stage-1", 1500, 1, &clock_at(&now))
        .expect("first reservation must succeed");
    assert_eq!(receipt.sequence, 0);
    assert_eq!(receipt.caller_id, "stage-1");
    assert_eq!(receipt.input_bytes, 1500);
    assert_eq!(receipt.format_attempt, 1);
    assert_eq!(
        receipt.usage,
        BudgetUsage {
            used_calls: 1,
            max_calls: 10,
            used_input_bytes: 1500,
            max_input_bytes: 10_000,
        }
    );
    // A format repair is a separate model attempt: charged cumulatively.
    let repair = ledger
        .reserve("stage-1-repair", 500, 2, &clock_at(&now))
        .expect("repair reservation must succeed");
    assert_eq!(repair.sequence, 1);
    assert_eq!(repair.format_attempt, 2);
    assert_eq!(ledger.usage().used_calls, 2);
    assert_eq!(ledger.usage().used_input_bytes, 2000);
}

#[test]
fn consume_returns_metadata_exactly_once_and_rejects_replay() {
    let mut ledger = new_ledger(10, 10_000);
    let now = Cell::new(START_MS);
    let receipt = ledger
        .reserve("stage-1", 1500, 1, &clock_at(&now))
        .expect("reservation must succeed");
    now.set(START_MS + 1000);
    let consumed = ledger
        .consume(receipt.sequence, 1500, 1, &clock_at(&now))
        .expect("first consume must succeed");
    assert_eq!(consumed.sequence, receipt.sequence);
    assert_eq!(consumed.caller_id, "stage-1");
    assert_eq!(consumed.input_bytes, 1500);
    assert_eq!(consumed.format_attempt, 1);
    // Replay of the same sequence never succeeds again.
    assert_eq!(
        ledger.consume(receipt.sequence, 1500, 1, &clock_at(&now)),
        Err(BudgetError::AlreadyConsumed)
    );
    // Unknown sequences never succeed.
    assert_eq!(
        ledger.consume(999, 1500, 1, &clock_at(&now)),
        Err(BudgetError::UnknownSequence)
    );
    // Consumption does not refund the charge.
    assert_eq!(ledger.usage().used_calls, 1);
    assert_eq!(ledger.usage().used_input_bytes, 1500);
}

#[test]
fn consume_rejects_mismatched_expectations_without_side_effects() {
    let mut ledger = new_ledger(10, 10_000);
    let now = Cell::new(START_MS);
    let receipt = ledger
        .reserve("stage-1", 1500, 1, &clock_at(&now))
        .expect("reservation must succeed");
    let clock_reads = Cell::new(0u32);
    let counting_clock = || {
        clock_reads.set(clock_reads.get() + 1);
        Ok(now.get())
    };
    assert_eq!(
        ledger.consume(receipt.sequence, 999, 1, &counting_clock),
        Err(BudgetError::ReservationMismatch),
        "wrong byte count"
    );
    assert_eq!(
        ledger.consume(receipt.sequence, 1500, 2, &counting_clock),
        Err(BudgetError::ReservationMismatch),
        "wrong format attempt"
    );
    assert_eq!(
        ledger.consume(999, 1500, 1, &counting_clock),
        Err(BudgetError::UnknownSequence),
        "unknown sequence"
    );
    assert_eq!(
        clock_reads.get(),
        0,
        "invalid consume preflight must not consult the clock"
    );
    // A rejected consume leaves the reservation redeemable exactly once.
    let consumed = ledger
        .consume(receipt.sequence, 1500, 1, &counting_clock)
        .expect("consume with matching expectations must succeed");
    assert_eq!(consumed.sequence, receipt.sequence);
    assert_eq!(
        ledger.consume(receipt.sequence, 1500, 1, &clock_at(&now)),
        Err(BudgetError::AlreadyConsumed)
    );
}

#[test]
fn call_and_input_limits_reject_explicitly_including_format_repair() {
    let now = Cell::new(START_MS);
    // Input bytes: 600 charged, the 500-byte repair would exceed 1000.
    let mut ledger = new_ledger(10, 1000);
    ledger
        .reserve("stage-1", 600, 1, &clock_at(&now))
        .expect("first reservation must succeed");
    assert_eq!(
        ledger.reserve("stage-1-repair", 500, 2, &clock_at(&now)),
        Err(BudgetError::InputLimitExceeded),
        "repair attempts count against the same input budget"
    );
    assert_eq!(
        ledger.usage().used_input_bytes,
        600,
        "a rejected reservation never charges"
    );
    // Exactly the remaining budget is admitted.
    ledger
        .reserve("stage-2", 400, 1, &clock_at(&now))
        .expect("reservation up to the limit must succeed");
    assert_eq!(ledger.usage().used_input_bytes, 1000);
    assert_eq!(
        ledger.reserve("stage-3", 1, 1, &clock_at(&now)),
        Err(BudgetError::InputLimitExceeded)
    );

    // Calls: the third reservation exceeds max_calls=2; the repair consumed
    // one of the two calls.
    let mut ledger = new_ledger(2, MAX_LEDGER_INPUT_BYTES);
    ledger
        .reserve("a", 10, 1, &clock_at(&now))
        .expect("first call must succeed");
    ledger
        .reserve("a-repair", 10, 2, &clock_at(&now))
        .expect("second call (format repair) must succeed");
    assert_eq!(
        ledger.reserve("b", 10, 1, &clock_at(&now)),
        Err(BudgetError::CallLimitExceeded)
    );
    assert_eq!(ledger.usage().used_calls, 2);
}

#[test]
fn rejected_invalid_preflight_leaves_ledger_unchanged() {
    // Large enough to admit the 4 MiB boundary reservation below.
    let mut ledger = new_ledger(10, MAX_LEDGER_INPUT_BYTES);
    let now = Cell::new(START_MS);
    let clock_reads = Cell::new(0u32);
    let counting_clock = || {
        clock_reads.set(clock_reads.get() + 1);
        Ok(now.get())
    };
    let long_id = "x".repeat(MAX_CALLER_ID_BYTES + 1);
    for invalid_id in ["", "  ", long_id.as_str(), "stage\0x", " stage-1"] {
        assert_eq!(
            ledger.reserve(invalid_id, 100, 1, &counting_clock),
            Err(BudgetError::InvalidCallerId),
            "{invalid_id:?}"
        );
    }
    for invalid_bytes in [0, MAX_RESERVATION_INPUT_BYTES + 1] {
        assert_eq!(
            ledger.reserve("stage-1", invalid_bytes, 1, &counting_clock),
            Err(BudgetError::InvalidInputBytes),
            "{invalid_bytes}"
        );
    }
    for invalid_attempt in [0, 3, u8::MAX] {
        assert_eq!(
            ledger.reserve("stage-1", 100, invalid_attempt, &counting_clock),
            Err(BudgetError::InvalidFormatAttempt),
            "{invalid_attempt}"
        );
    }
    assert_eq!(
        clock_reads.get(),
        0,
        "preflight rejections must not consult the clock"
    );
    // Duplicate caller id: the first reservation is charged exactly once.
    ledger
        .reserve("stage-1", 100, 1, &counting_clock)
        .expect("reservation must succeed");
    assert_eq!(
        ledger.reserve("stage-1", 100, 1, &counting_clock),
        Err(BudgetError::DuplicateCaller)
    );
    assert_eq!(
        clock_reads.get(),
        2,
        "the successful reservation consulted the clock twice (initial and final checks)"
    );
    assert_eq!(
        ledger.usage(),
        BudgetUsage {
            used_calls: 1,
            max_calls: 10,
            used_input_bytes: 100,
            max_input_bytes: MAX_LEDGER_INPUT_BYTES,
        }
    );
    // Exact boundaries are admitted: id at the byte bound, 4 MiB input.
    let at_bounds = ledger.reserve(
        &"y".repeat(MAX_CALLER_ID_BYTES),
        MAX_RESERVATION_INPUT_BYTES,
        2,
        &clock_at(&now),
    );
    assert!(at_bounds.is_ok(), "boundary values must be admitted");
    assert_eq!(ledger.usage().used_calls, 2);
}

#[test]
fn unused_reservation_stays_charged_and_expires_with_ledger() {
    let mut ledger = new_ledger(10, 10_000);
    let now = Cell::new(START_MS);
    ledger
        .reserve("never-used", 700, 1, &clock_at(&now))
        .expect("reservation must succeed");
    assert_eq!(
        ledger.usage().used_input_bytes,
        700,
        "unused reservations are never refunded"
    );
    // The deadline is exclusive: the last valid instant is deadline - 1.
    now.set(START_MS + DURATION_MS as i64 - 1);
    ledger
        .reserve("last-instant", 100, 1, &clock_at(&now))
        .expect("deadline - 1 is still valid");
    // At the deadline the ledger closes permanently; charges are preserved.
    now.set(START_MS + DURATION_MS as i64);
    let expired = ledger.reserve("too-late", 100, 1, &clock_at(&now));
    match expired {
        Err(BudgetError::Closed(reason)) => {
            assert!(
                reason.contains("deadline"),
                "expiry reason must name the deadline, got {reason:?}"
            );
        }
        other => panic!("expected permanent close at the deadline, got {other:?}"),
    }
    let usage = ledger.usage();
    assert_eq!(usage.used_calls, 2);
    assert_eq!(usage.used_input_bytes, 800);
    // Everything fails now, including consuming either reservation.
    assert!(matches!(
        ledger.consume(0, 700, 1, &clock_at(&now)),
        Err(BudgetError::Closed(_))
    ));
    assert!(matches!(
        ledger.consume(1, 100, 1, &clock_at(&now)),
        Err(BudgetError::Closed(_))
    ));
}

#[test]
fn failed_final_check_preserves_charge_and_never_returns_lease() {
    let mut ledger = new_ledger(10, 10_000);
    let now = Cell::new(START_MS);
    let receipt = ledger
        .reserve("stage-1", 900, 1, &clock_at(&now))
        .expect("reservation must succeed");
    // The attempt ran; by redemption time the budget window has expired.
    now.set(START_MS + DURATION_MS as i64);
    let revoked = ledger.consume(receipt.sequence, 900, 1, &clock_at(&now));
    assert!(
        matches!(revoked, Err(BudgetError::Closed(_))),
        "no lease metadata may be returned after a failed final check: {revoked:?}"
    );
    // The charge is kept, never refunded.
    assert_eq!(ledger.usage().used_calls, 1);
    assert_eq!(ledger.usage().used_input_bytes, 900);
    // The close is permanent: retrying at a valid time still fails.
    now.set(START_MS + 1);
    assert!(matches!(
        ledger.consume(receipt.sequence, 900, 1, &clock_at(&now)),
        Err(BudgetError::Closed(_))
    ));
}

#[test]
fn reserve_final_callback_cancellation_closes_permanently_and_keeps_charge() {
    let mut ledger = new_ledger(10, 10_000);
    let reads = Cell::new(0u32);
    let message = "grant revoked for persona-9 while the attempt ran";
    let clock = || {
        let read = reads.get() + 1;
        reads.set(read);
        if read == 1 {
            Ok(START_MS)
        } else {
            Err(message.to_string())
        }
    };
    let receipt = ledger.reserve("stage-1", 900, 1, &clock);
    match receipt {
        Err(BudgetError::Closed(reason)) => assert_eq!(
            reason, message,
            "the final callback reason is preserved byte-for-byte"
        ),
        other => panic!("no receipt may be returned after a failed final check: {other:?}"),
    }
    assert_eq!(reads.get(), 2, "initial and final checks both ran");
    // The charge happened before the final check and is never refunded.
    assert_eq!(ledger.usage().used_calls, 1);
    assert_eq!(ledger.usage().used_input_bytes, 900);
    // The close is permanent and keeps the exact reason.
    assert_eq!(ledger.closed_reason(), Some(message));
    let later = Cell::new(START_MS);
    assert!(matches!(
        ledger.consume(0, 900, 1, &clock_at(&later)),
        Err(BudgetError::Closed(ref reason)) if reason == message
    ));
    assert!(matches!(
        ledger.reserve("stage-2", 100, 1, &clock_at(&later)),
        Err(BudgetError::Closed(ref reason)) if reason == message
    ));
}

#[test]
fn reserve_final_callback_expiry_closes_permanently_and_keeps_charge() {
    let mut ledger = new_ledger(10, 10_000);
    let reads = Cell::new(0u32);
    let clock = || {
        let read = reads.get() + 1;
        reads.set(read);
        if read == 1 {
            Ok(START_MS)
        } else {
            Ok(START_MS + DURATION_MS as i64)
        }
    };
    let receipt = ledger.reserve("stage-1", 900, 1, &clock);
    let expected = format!(
        "model call ledger closed: wall time {} reached the exclusive deadline {}",
        START_MS + DURATION_MS as i64,
        START_MS + DURATION_MS as i64
    );
    match receipt {
        Err(BudgetError::Closed(reason)) => assert_eq!(reason, expected),
        other => panic!("no receipt may be returned after expiry during work: {other:?}"),
    }
    assert_eq!(reads.get(), 2, "initial and final checks both ran");
    assert_eq!(ledger.usage().used_calls, 1);
    assert_eq!(ledger.usage().used_input_bytes, 900);
    assert_eq!(ledger.closed_reason(), Some(expected.as_str()));
    let later = Cell::new(START_MS + 1);
    assert!(matches!(
        ledger.reserve("stage-2", 100, 1, &clock_at(&later)),
        Err(BudgetError::Closed(ref reason)) if reason == expected.as_str()
    ));
}

#[test]
fn consume_final_callback_cancellation_closes_permanently_and_keeps_charge() {
    let mut ledger = new_ledger(10, 10_000);
    let now = Cell::new(START_MS);
    let receipt = ledger
        .reserve("stage-1", 900, 1, &clock_at(&now))
        .expect("reservation must succeed");
    let reads = Cell::new(0u32);
    let message = "lease revoked by host registry during consumption";
    let clock = || {
        let read = reads.get() + 1;
        reads.set(read);
        if read == 1 {
            Ok(START_MS + 1)
        } else {
            Err(message.to_string())
        }
    };
    let consumed = ledger.consume(receipt.sequence, 900, 1, &clock);
    match consumed {
        Err(BudgetError::Closed(reason)) => assert_eq!(
            reason, message,
            "the final callback reason is preserved byte-for-byte"
        ),
        other => panic!("no lease metadata may be returned after a failed final check: {other:?}"),
    }
    assert_eq!(reads.get(), 2, "initial and final checks both ran");
    assert_eq!(ledger.usage().used_calls, 1);
    assert_eq!(ledger.usage().used_input_bytes, 900);
    assert_eq!(ledger.closed_reason(), Some(message));
    // Permanent: even a correct retry at a valid time fails with the same reason.
    now.set(START_MS + 2);
    assert!(matches!(
        ledger.consume(receipt.sequence, 900, 1, &clock_at(&now)),
        Err(BudgetError::Closed(ref reason)) if reason == message
    ));
}

#[test]
fn consume_final_callback_expiry_closes_permanently_and_keeps_charge() {
    let mut ledger = new_ledger(10, 10_000);
    let now = Cell::new(START_MS);
    let receipt = ledger
        .reserve("stage-1", 900, 1, &clock_at(&now))
        .expect("reservation must succeed");
    let reads = Cell::new(0u32);
    let clock = || {
        let read = reads.get() + 1;
        reads.set(read);
        if read == 1 {
            Ok(START_MS + 1)
        } else {
            Ok(START_MS + DURATION_MS as i64)
        }
    };
    let consumed = ledger.consume(receipt.sequence, 900, 1, &clock);
    let expected = format!(
        "model call ledger closed: wall time {} reached the exclusive deadline {}",
        START_MS + DURATION_MS as i64,
        START_MS + DURATION_MS as i64
    );
    match consumed {
        Err(BudgetError::Closed(reason)) => assert_eq!(reason, expected),
        other => panic!("no lease metadata may be returned after expiry during work: {other:?}"),
    }
    assert_eq!(reads.get(), 2, "initial and final checks both ran");
    assert_eq!(ledger.usage().used_calls, 1);
    assert_eq!(ledger.usage().used_input_bytes, 900);
    assert_eq!(ledger.closed_reason(), Some(expected.as_str()));
}

#[test]
fn backward_clock_closes_permanently_with_exact_reason() {
    let mut ledger = new_ledger(10, 10_000);
    let now = Cell::new(START_MS);
    ledger
        .reserve("stage-1", 100, 1, &clock_at(&now))
        .expect("reservation must succeed");
    now.set(START_MS - 1);
    let backward = ledger.consume(0, 100, 1, &clock_at(&now));
    match backward {
        Err(BudgetError::Closed(reason)) => {
            assert!(
                reason.contains("backward"),
                "reason must name the backwards move, got {reason:?}"
            );
            assert!(
                !reason.contains("sentinel"),
                "reason must not be sentinel-classified"
            );
        }
        other => panic!("expected permanent close, got {other:?}"),
    }
    // Later valid times stay closed; the earlier charge is preserved.
    now.set(START_MS);
    assert!(matches!(
        ledger.reserve("stage-2", 100, 1, &clock_at(&now)),
        Err(BudgetError::Closed(_))
    ));
    assert_eq!(ledger.usage().used_calls, 1);
}

#[test]
fn negative_clock_closes_permanently() {
    let mut ledger = new_ledger(10, 10_000);
    let rejected = ledger.reserve("stage-1", 100, 1, &|| Ok(-5));
    match rejected {
        Err(BudgetError::Closed(reason)) => {
            assert!(
                reason.contains("negative"),
                "reason must name the negative reading, got {reason:?}"
            );
        }
        other => panic!("expected permanent close, got {other:?}"),
    }
    assert!(ledger.closed_reason().is_some());
}

#[test]
fn clock_error_preserves_exact_callback_reason_without_sentinels() {
    let mut ledger = new_ledger(10, 10_000);
    let message = "grant revoked for persona-9: scope changed by host";
    let failed = ledger.reserve("stage-1", 100, 1, &|| Err(message.to_string()));
    match failed {
        Err(BudgetError::Closed(reason)) => {
            assert_eq!(
                reason, message,
                "host reason is preserved byte-for-byte, never sentinel-classified"
            );
        }
        other => panic!("expected closed ledger, got {other:?}"),
    }
    // The same exact reason is reported by every later operation.
    assert!(matches!(
        ledger.consume(0, 100, 1, &|| Ok(0)),
        Err(BudgetError::Closed(ref reason)) if reason == message
    ));
    assert_eq!(ledger.closed_reason(), Some(message));
}

#[test]
fn reservations_have_unique_bounded_sequences() {
    let mut ledger = new_ledger(5, 1_000_000);
    let now = Cell::new(START_MS);
    let mut seen = std::collections::HashSet::new();
    for index in 0..5u64 {
        let receipt = ledger
            .reserve(&format!("stage-{index}"), 100, 1, &clock_at(&now))
            .expect("reservation must succeed");
        assert!(
            seen.insert(receipt.sequence),
            "sequences must be unique, got {} twice",
            receipt.sequence
        );
    }
    assert_eq!(
        ledger.reserve("overflow", 100, 1, &clock_at(&now)),
        Err(BudgetError::CallLimitExceeded)
    );
    assert_eq!(
        ledger.usage().used_calls,
        5,
        "records are inherently bounded by max_calls"
    );
    for sequence in seen {
        assert!(
            ledger.consume(sequence, 100, 1, &clock_at(&now)).is_ok(),
            "reserved sequence {sequence} must be redeemable exactly once"
        );
    }
}

#[test]
fn close_is_idempotent_and_all_later_operations_fail() {
    let mut ledger = new_ledger(10, 10_000);
    let now = Cell::new(START_MS);
    let receipt = ledger
        .reserve("stage-1", 100, 1, &clock_at(&now))
        .expect("reservation must succeed");
    assert!(ledger.close("host shutdown requested"));
    assert!(
        !ledger.close("second reason is ignored"),
        "close must be idempotent and keep the first reason"
    );
    assert_eq!(ledger.closed_reason(), Some("host shutdown requested"));
    assert!(matches!(
        ledger.reserve("stage-2", 100, 1, &clock_at(&now)),
        Err(BudgetError::Closed(ref reason)) if reason == "host shutdown requested"
    ));
    assert!(matches!(
        ledger.consume(receipt.sequence, 100, 1, &clock_at(&now)),
        Err(BudgetError::Closed(_))
    ));
    // Charges taken before the close remain visible.
    assert_eq!(ledger.usage().used_calls, 1);
}

#[test]
fn time_never_moves_backwards_between_successful_operations() {
    let mut ledger = new_ledger(10, 10_000);
    let now = Cell::new(START_MS);
    ledger
        .reserve("stage-1", 100, 1, &clock_at(&now))
        .expect("reservation must succeed");
    now.set(START_MS + 5);
    assert!(ledger.reserve("stage-2", 100, 1, &clock_at(&now)).is_ok());
    // Equal time is not backwards and stays admissible.
    assert!(ledger.reserve("stage-3", 100, 1, &clock_at(&now)).is_ok());
}

#[test]
fn completion_checks_preserve_charges_and_do_not_consume_another_reservation() {
    let mut ledger = ModelCallLedger::new(2, 100, 100, 10).unwrap();
    let reservation = ledger.reserve("call", 20, 1, &|| Ok(10)).unwrap();
    let usage = ledger.usage();
    assert_eq!(ledger.verify(&|| Ok(20)).unwrap(), 20);
    assert_eq!(ledger.usage(), usage);
    ledger
        .consume(reservation.sequence, 20, 1, &|| Ok(21))
        .unwrap();
    assert_eq!(ledger.verify(&|| Ok(22)).unwrap(), 22);
    assert_eq!(ledger.usage(), usage);
    assert!(matches!(
        ledger.verify(&|| Ok(110)),
        Err(BudgetError::Closed(_))
    ));
    assert!(matches!(
        ledger.verify(&|| panic!("expired ledger read clock")),
        Err(BudgetError::Closed(_))
    ));
}
#[test]
fn failed_or_backwards_completion_check_closes_the_ledger_without_refund() {
    for revoke in [true, false] {
        let mut ledger = ModelCallLedger::new(2, 100, 100, 10).unwrap();
        let reservation = ledger.reserve("call", 20, 1, &|| Ok(20)).unwrap();
        let usage = ledger.usage();
        let checked = ledger.verify(&|| {
            if revoke {
                Err("grant revoked\r\n".into())
            } else {
                Ok(19)
            }
        });
        assert!(matches!(checked, Err(BudgetError::Closed(_))));
        if revoke {
            assert_eq!(ledger.closed_reason(), Some("grant revoked\r\n"));
        }
        assert_eq!(ledger.usage(), usage);
        assert!(matches!(
            ledger.consume(reservation.sequence, 20, 1, &|| panic!(
                "closed ledger reached consume clock"
            )),
            Err(BudgetError::Closed(_))
        ));
    }
}
