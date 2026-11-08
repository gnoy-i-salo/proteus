//! Isolated std-only tests for the native runtime-resource slot.
//!
//! These tests use a small fake runtime with drop counters only. No E5 data,
//! no model bytes, no filesystem and no network: the slot never inspects any
//! of those. Concurrency is coordinated with channels and rendezvous barriers;
//! every cross-thread wait is bounded by `RECV_WAIT`, never by hoping a sleep
//! lands between two scheduler decisions.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

const RESERVATION: u64 = 1536 * 1024 * 1024;
const RECV_WAIT: Duration = Duration::from_secs(5);

/// Test double standing in for a verified heavyweight runtime.
struct FakeRuntime {
    label: &'static str,
    drops: Arc<AtomicUsize>,
}
impl Drop for FakeRuntime {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

/// Runtime whose destructor re-enters the slot, like a heavyweight native
/// runtime querying `status` from its `Drop` impl. Exercises the rule that
/// the last-handle drop must never run under the metadata lock.
struct ReentrantRuntime {
    slot: RuntimeSlot<ReentrantRuntime>,
    drops: Arc<AtomicUsize>,
}
impl Drop for ReentrantRuntime {
    fn drop(&mut self) {
        let _ = self.slot.status();
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

/// Work result whose destructor re-enters the slot; discarded results (late
/// cancellation) must be dropped outside the metadata lock too.
struct ReentrantResult {
    slot: RuntimeSlot<FakeRuntime>,
    drops: Arc<AtomicUsize>,
}
impl Drop for ReentrantResult {
    fn drop(&mut self) {
        let _ = self.slot.status();
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

/// Shared counters proving exactly which host callback ran how often.
struct CallbackCounters {
    loads: AtomicUsize,
    works: AtomicUsize,
    load_errors: AtomicUsize,
}
impl CallbackCounters {
    fn new() -> Self {
        Self {
            loads: AtomicUsize::new(0),
            works: AtomicUsize::new(0),
            load_errors: AtomicUsize::new(0),
        }
    }
    fn load_count(&self) -> usize {
        self.loads.load(Ordering::SeqCst)
    }
    fn work_count(&self) -> usize {
        self.works.load(Ordering::SeqCst)
    }
}

/// One-shot rendezvous: the callback announces "entered" and then blocks until
/// the test thread sends "release". This replaces sleep-based scheduling.
struct Gate {
    entered: Sender<()>,
    release: Receiver<()>,
}
impl Gate {
    fn pair() -> (Self, GateProbe) {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        (
            Self {
                entered: entered_tx,
                release: release_rx,
            },
            GateProbe {
                entered: entered_rx,
                release: release_tx,
            },
        )
    }
    fn announce_and_block(&self) {
        self.entered.send(()).expect("test thread must be waiting");
        self.release.recv().expect("test thread must release");
    }
}
struct GateProbe {
    entered: Receiver<()>,
    release: Sender<()>,
}
impl GateProbe {
    fn wait_entered(&self) {
        self.entered
            .recv_timeout(RECV_WAIT)
            .expect("callback must enter within bounded wait");
    }
    fn release(self) {
        self.release.send(()).expect("callback must be blocked");
    }
}

/// Bounded join: the worker reports its `run` result over a channel so the
/// test thread never blocks forever on a wedged slot.
struct BoundedWorker<R> {
    handle: JoinHandle<()>,
    result: Receiver<R>,
}
fn spawn_bounded<R, F>(body: F) -> BoundedWorker<R>
where
    R: Send + 'static,
    F: FnOnce() -> R + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let result = body();
        tx.send(result).expect("test thread must collect result");
    });
    BoundedWorker { handle, result: rx }
}
impl<R> BoundedWorker<R> {
    fn join_bounded(self) -> R {
        let result = self
            .result
            .recv_timeout(RECV_WAIT)
            .expect("worker must finish within bounded wait");
        self.handle.join().expect("worker must not panic");
        result
    }
}

fn new_slot() -> RuntimeSlot<FakeRuntime> {
    RuntimeSlot::new(RESERVATION).expect("positive reservation must construct")
}

fn ok_loader(
    counters: Arc<CallbackCounters>,
    drops: Arc<AtomicUsize>,
) -> impl Fn(&CancelToken) -> Result<FakeRuntime, String> {
    move |_| {
        counters.loads.fetch_add(1, Ordering::SeqCst);
        Ok(FakeRuntime {
            label: "verified",
            drops: drops.clone(),
        })
    }
}

fn ok_work(
    counters: Arc<CallbackCounters>,
) -> impl Fn(&FakeRuntime, &CancelToken) -> Result<usize, String> {
    move |runtime, _| {
        counters.works.fetch_add(1, Ordering::SeqCst);
        Ok(runtime.label.len())
    }
}

#[test]
fn constructor_fixes_reservation_and_rejects_unsafe_values() {
    for invalid in [0, MAX_RESIDENT_BYTES + 1] {
        assert!(
            matches!(
                RuntimeSlot::<FakeRuntime>::new(invalid),
                Err(SlotError::InvalidBudget)
            ),
            "{invalid}"
        );
    }
    let slot = new_slot();
    assert_eq!(slot.resident_reservation_bytes(), RESERVATION);
    // The reservation is fixed by native construction; there is no mutator.
    let status = slot.status();
    assert_eq!(status.resident_reservation_bytes, RESERVATION);
}

#[test]
fn status_tracks_cached_runtime_not_disk_or_package_state() {
    let slot = new_slot();
    // Absent/Ready describe the in-memory cached runtime only; whether the
    // model package exists on disk is the cache/manager owner's status and is
    // intentionally not reported here.
    assert_eq!(
        slot.status(),
        SlotStatus {
            cached_runtime: CachedRuntime::Absent,
            operation_active: false,
            resident_reservation_bytes: RESERVATION,
        }
    );
}

#[test]
fn run_rejects_invalid_operation_ids_before_callbacks() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let long_id = "x".repeat(MAX_OPERATION_ID_BYTES + 1);
    for invalid in ["", "   ", long_id.as_str(), "op\0x", "op\u{009f}x"] {
        let result: Result<usize, SlotError<String>> = slot.run(
            invalid,
            RESERVATION,
            ok_loader(counters.clone(), drops.clone()),
            ok_work(counters.clone()),
        );
        assert_eq!(result, Err(SlotError::InvalidOperationId), "{invalid:?}");
    }
    assert_eq!(counters.load_count(), 0);
    assert_eq!(counters.work_count(), 0);
    // Exactly at the bound is admitted.
    let exact = "x".repeat(MAX_OPERATION_ID_BYTES);
    let admitted: Result<usize, SlotError<String>> = slot.run(
        exact.as_str(),
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(admitted, Ok("verified".len()));
    // Ids are opaque: padding is rejected, never trimmed into a valid id, on
    // both `run` and `cancel`.
    let padded = format!("  {exact}  ");
    let padded_result: Result<usize, SlotError<String>> = slot.run(
        padded.as_str(),
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(padded_result, Err(SlotError::InvalidOperationId));
    assert_eq!(
        slot.cancel(padded.as_str()),
        Err(SlotError::InvalidOperationId)
    );
    assert_eq!(counters.load_count(), 1);
    assert_eq!(counters.work_count(), 1);
}

#[test]
fn run_rejects_invalid_and_under_reservation_budgets_before_callbacks() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    for (budget, expected) in [
        (0, SlotError::InvalidBudget),
        (MAX_RESIDENT_BYTES + 1, SlotError::InvalidBudget),
        (RESERVATION - 1, SlotError::ResidentReservation),
    ] {
        let result: Result<usize, SlotError<String>> = slot.run(
            "op-budget",
            budget,
            ok_loader(counters.clone(), drops.clone()),
            ok_work(counters.clone()),
        );
        assert_eq!(result, Err(expected), "{budget}");
    }
    assert_eq!(counters.load_count(), 0);
    assert_eq!(counters.work_count(), 0);
}

#[test]
fn cached_runtime_is_reused_without_second_loader_invocation() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    for _ in 0..2 {
        let result: Result<usize, SlotError<String>> = slot.run(
            "op-reuse",
            RESERVATION,
            ok_loader(counters.clone(), drops.clone()),
            ok_work(counters.clone()),
        );
        assert_eq!(result, Ok("verified".len()));
    }
    assert_eq!(counters.load_count(), 1);
    assert_eq!(counters.work_count(), 2);
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Ready);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
}

#[test]
fn lower_resident_budget_is_rejected_even_when_runtime_is_cached() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let first: Result<usize, SlotError<String>> = slot.run(
        "op-warm",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(first, Ok("verified".len()));
    let second: Result<usize, SlotError<String>> = slot.run(
        "op-cheap",
        RESERVATION - 1,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(second, Err(SlotError::ResidentReservation));
    assert_eq!(counters.load_count(), 1);
    assert_eq!(counters.work_count(), 1);
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Ready);
}

#[test]
fn concurrent_operation_fails_busy_before_any_callback() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let (gate, probe) = Gate::pair();
    let first_slot = slot.clone();
    let first_counters = counters.clone();
    let first_drops = drops.clone();
    let work_counters = first_counters.clone();
    let first = spawn_bounded(move || {
        let result: Result<usize, SlotError<String>> = first_slot.run(
            "op-first",
            RESERVATION,
            move |_| {
                first_counters.loads.fetch_add(1, Ordering::SeqCst);
                gate.announce_and_block();
                Ok(FakeRuntime {
                    label: "verified",
                    drops: first_drops,
                })
            },
            ok_work(work_counters),
        );
        result
    });
    probe.wait_entered();
    // A cloned handle shares the same slot: the second operation must fail
    // Busy without invoking either of its callbacks.
    let second_counters = Arc::new(CallbackCounters::new());
    let second_drops = Arc::new(AtomicUsize::new(0));
    let second = spawn_bounded({
        let slot = slot.clone();
        let counters = second_counters.clone();
        let drops = second_drops.clone();
        move || {
            let result: Result<usize, SlotError<String>> = slot.run(
                "op-second",
                RESERVATION,
                ok_loader(counters, drops),
                ok_work(second_counters),
            );
            result
        }
    });
    assert_eq!(
        second.join_bounded(),
        Err(SlotError::Busy),
        "queued work would defeat the single-operation bound"
    );
    probe.release();
    assert_eq!(first.join_bounded(), Ok("verified".len()));
    assert_eq!(counters.load_count(), 1);
    assert_eq!(counters.work_count(), 1);
}

#[test]
fn cancel_during_load_prevents_publish_and_work() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let (gate, probe) = Gate::pair();
    let cancelled_seen_by_loader = Arc::new(AtomicBool::new(false));
    let seen = cancelled_seen_by_loader.clone();
    let worker = spawn_bounded({
        let slot = slot.clone();
        let counters = counters.clone();
        let drops = drops.clone();
        let work_counters = counters.clone();
        move || {
            let result: Result<usize, SlotError<String>> = slot.run(
                "op-load",
                RESERVATION,
                move |token| {
                    counters.loads.fetch_add(1, Ordering::SeqCst);
                    gate.announce_and_block();
                    seen.store(token.is_cancelled(), Ordering::SeqCst);
                    Ok(FakeRuntime {
                        label: "verified",
                        drops: drops.clone(),
                    })
                },
                ok_work(work_counters),
            );
            result
        }
    });
    probe.wait_entered();
    assert_eq!(slot.cancel("op-load"), Ok(true));
    // Cooperative cancellation: the slot stays reserved and busy until the
    // callback actually returns.
    assert!(slot.status().operation_active);
    probe.release();
    assert_eq!(worker.join_bounded(), Err(SlotError::Cancelled));
    assert!(cancelled_seen_by_loader.load(Ordering::SeqCst));
    assert_eq!(counters.work_count(), 0);
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Absent);
    // The discarded runtime was dropped, never published.
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn late_cancel_during_work_discards_success_value_but_keeps_cache() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let warm: Result<usize, SlotError<String>> = slot.run(
        "op-warm",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(warm, Ok("verified".len()));
    let (gate, probe) = Gate::pair();
    let worker = spawn_bounded({
        let slot = slot.clone();
        let counters = counters.clone();
        let drops = drops.clone();
        move || {
            let result: Result<usize, SlotError<String>> = slot.run(
                "op-late",
                RESERVATION,
                ok_loader(counters.clone(), drops),
                move |runtime, _| {
                    counters.works.fetch_add(1, Ordering::SeqCst);
                    gate.announce_and_block();
                    Ok(runtime.label.len())
                },
            );
            result
        }
    });
    probe.wait_entered();
    assert_eq!(slot.cancel("op-late"), Ok(true));
    probe.release();
    // The work completed, but a late cancellation must not surface as success.
    assert_eq!(worker.join_bounded(), Err(SlotError::Cancelled));
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Ready);
    // Retry reuses the cached runtime: the loader still ran exactly once.
    let retry: Result<usize, SlotError<String>> = slot.run(
        "op-retry",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(retry, Ok("verified".len()));
    assert_eq!(counters.load_count(), 1);
}

#[test]
fn stale_operation_id_cannot_cancel_the_next_operation() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let finished: Result<usize, SlotError<String>> = slot.run(
        "op-stale",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(finished, Ok("verified".len()));
    let (gate, probe) = Gate::pair();
    let worker = spawn_bounded({
        let slot = slot.clone();
        let counters = counters.clone();
        let drops = drops.clone();
        move || {
            let result: Result<usize, SlotError<String>> = slot.run(
                "op-current",
                RESERVATION,
                ok_loader(counters, drops),
                move |runtime, _| {
                    gate.announce_and_block();
                    Ok(runtime.label.len())
                },
            );
            result
        }
    });
    probe.wait_entered();
    // The stale id is well-formed but matches no active operation.
    assert_eq!(slot.cancel("op-stale"), Ok(false));
    probe.release();
    assert_eq!(worker.join_bounded(), Ok("verified".len()));
}

#[test]
fn busy_is_retained_after_cancel_until_the_callback_exits() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let (gate, probe) = Gate::pair();
    let first = spawn_bounded({
        let slot = slot.clone();
        let counters = counters.clone();
        let drops = drops.clone();
        move || {
            let result: Result<usize, SlotError<String>> = slot.run(
                "op-holding",
                RESERVATION,
                ok_loader(counters, drops),
                move |runtime, _| {
                    gate.announce_and_block();
                    Ok(runtime.label.len())
                },
            );
            result
        }
    });
    probe.wait_entered();
    assert_eq!(slot.cancel("op-holding"), Ok(true));
    // Cancellation is not admission: the next operation must still fail Busy
    // while the cancelled callback is blocked inside the slot.
    let contender = spawn_bounded({
        let slot = slot.clone();
        let counters = counters.clone();
        let drops = drops.clone();
        move || {
            let result: Result<usize, SlotError<String>> = slot.run(
                "op-contender",
                RESERVATION,
                ok_loader(counters, drops),
                ok_work(Arc::new(CallbackCounters::new())),
            );
            result
        }
    });
    assert_eq!(contender.join_bounded(), Err(SlotError::Busy));
    probe.release();
    assert_eq!(first.join_bounded(), Err(SlotError::Cancelled));
    let after: Result<usize, SlotError<String>> = slot.run(
        "op-after",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(after, Ok("verified".len()));
}

#[test]
fn failed_loader_frees_slot_and_preserves_exact_error_payload() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let failed: Result<usize, SlotError<String>> = slot.run(
        "op-fail",
        RESERVATION,
        |_| {
            counters.load_errors.fetch_add(1, Ordering::SeqCst);
            Err("checksum mismatch: weights".to_string())
        },
        ok_work(counters.clone()),
    );
    assert_eq!(
        failed,
        Err(SlotError::LoadFailed(
            "checksum mismatch: weights".to_string()
        )),
        "host error text must pass through verbatim, never sentinel-classified"
    );
    assert_eq!(counters.work_count(), 0);
    // A failed loader never becomes ready and leaves no active operation.
    let status = slot.status();
    assert_eq!(status.cached_runtime, CachedRuntime::Absent);
    assert!(!status.operation_active);
    let retry: Result<usize, SlotError<String>> = slot.run(
        "op-retry",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(retry, Ok("verified".len()));
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Ready);
}

#[test]
fn failed_work_returns_typed_payload_and_keeps_cached_runtime() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let warm: Result<usize, SlotError<String>> = slot.run(
        "op-warm",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(warm, Ok("verified".len()));
    let failed: Result<usize, SlotError<String>> = slot.run(
        "op-work-fail",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        |_, _| Err("inference exploded".to_string()),
    );
    assert_eq!(
        failed,
        Err(SlotError::WorkFailed("inference exploded".to_string()))
    );
    assert!(!slot.status().operation_active);
    // Publish happened before work ran; the verified runtime stays reusable.
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Ready);
    let retry: Result<usize, SlotError<String>> = slot.run(
        "op-retry",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(retry, Ok("verified".len()));
    assert_eq!(counters.load_count(), 1);
}

#[test]
fn unload_drops_cached_runtime_only_while_idle() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    assert_eq!(slot.unload(), Ok(false), "nothing cached yet");
    let warm: Result<usize, SlotError<String>> = slot.run(
        "op-warm",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(warm, Ok("verified".len()));
    let (gate, probe) = Gate::pair();
    let worker = spawn_bounded({
        let slot = slot.clone();
        let counters = counters.clone();
        let drops = drops.clone();
        move || {
            let result: Result<usize, SlotError<String>> = slot.run(
                "op-active",
                RESERVATION,
                ok_loader(counters, drops),
                move |runtime, _| {
                    gate.announce_and_block();
                    Ok(runtime.label.len())
                },
            );
            result
        }
    });
    probe.wait_entered();
    assert_eq!(slot.unload(), Err(SlotError::Busy));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    probe.release();
    assert_eq!(worker.join_bounded(), Ok("verified".len()));
    assert_eq!(slot.unload(), Ok(true));
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Absent);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "idle unload drops the runtime"
    );
}

#[test]
fn callback_can_query_status_and_cancel_without_deadlock() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let result: Result<usize, SlotError<String>> = slot.run(
        "op-self",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        |runtime, token| {
            // Re-entering the slot from inside a callback must be lock-free.
            assert!(!token.is_cancelled());
            assert_eq!(runtime.label, "verified");
            let status = slot.status();
            assert!(status.operation_active);
            assert_eq!(slot.cancel("op-self"), Ok(true));
            assert!(token.is_cancelled());
            Ok(runtime.label.len())
        },
    );
    // The callback finished successfully after requesting its own
    // cancellation, so the late cancellation discards the value.
    assert_eq!(result, Err(SlotError::Cancelled));
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Ready);
}

#[test]
fn unload_from_inside_callback_is_busy_not_deadlocked() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let result: Result<usize, SlotError<String>> = slot.run(
        "op-unload-self",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        |runtime, _| {
            assert_eq!(slot.unload(), Err(SlotError::Busy));
            Ok(runtime.label.len())
        },
    );
    assert_eq!(result, Ok("verified".len()));
}

#[test]
fn panic_in_callback_clears_busy_and_never_wedges_the_slot() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let panicking = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<usize, SlotError<String>> = slot.run(
            "op-panic-load",
            RESERVATION,
            |_| -> Result<FakeRuntime, String> { panic!("loader blew up") },
            ok_work(counters.clone()),
        );
    }));
    assert!(panicking.is_err());
    let status = slot.status();
    assert!(
        !status.operation_active,
        "RAII cleanup must clear the busy flag"
    );
    assert_eq!(status.cached_runtime, CachedRuntime::Absent);
    // The mutex was never held across the callback, so it is not poisoned:
    // ordinary operations continue to work.
    let recovered: Result<usize, SlotError<String>> = slot.run(
        "op-recover",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(recovered, Ok("verified".len()));
    // A panic after publish keeps the cached runtime and still frees the slot.
    let panicking_work = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<usize, SlotError<String>> = slot.run(
            "op-panic-work",
            RESERVATION,
            ok_loader(counters.clone(), drops.clone()),
            |_, _| -> Result<usize, String> { panic!("work blew up") },
        );
    }));
    assert!(panicking_work.is_err());
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Ready);
    assert!(!slot.status().operation_active);
}

#[test]
fn cancel_requires_a_valid_id_and_never_touches_an_inactive_slot() {
    let slot = new_slot();
    assert_eq!(slot.cancel(""), Err(SlotError::InvalidOperationId));
    assert_eq!(slot.cancel("no-such-operation"), Ok(false));
    assert!(!slot.status().operation_active);
}

/// The last-handle drop of the cached runtime runs a reentrant destructor
/// (it queries `status`). If `unload` dropped it under the metadata lock this
/// would deadlock; the bounded join proves the drop happens after the lock is
/// released.
#[test]
fn unload_drops_heavyweight_runtime_outside_the_metadata_lock() {
    let slot: RuntimeSlot<ReentrantRuntime> =
        RuntimeSlot::new(RESERVATION).expect("positive reservation must construct");
    let drops = Arc::new(AtomicUsize::new(0));
    let warm_slot = slot.clone();
    let warm_drops = drops.clone();
    let warm: Result<usize, SlotError<String>> = slot.run(
        "op-warm",
        RESERVATION,
        move |_| {
            Ok(ReentrantRuntime {
                slot: warm_slot.clone(),
                drops: warm_drops.clone(),
            })
        },
        |_, _| Ok(7),
    );
    assert_eq!(warm, Ok(7));
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Ready);
    let unloaded = spawn_bounded({
        let slot = slot.clone();
        move || slot.unload()
    });
    assert_eq!(unloaded.join_bounded(), Ok(true));
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Absent);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

/// A load cancelled mid-flight is discarded: the runtime is dropped (outside
/// the metadata lock, so a reentrant destructor stays deadlock-free), never
/// cached and `work` never runs.
#[test]
fn cancelled_load_discards_heavyweight_runtime_outside_the_lock() {
    let slot: RuntimeSlot<ReentrantRuntime> =
        RuntimeSlot::new(RESERVATION).expect("positive reservation must construct");
    let drops = Arc::new(AtomicUsize::new(0));
    let (gate, probe) = Gate::pair();
    let worker_slot = slot.clone();
    let worker_drops = drops.clone();
    let worker = spawn_bounded(move || {
        let runtime_slot = worker_slot.clone();
        let result: Result<usize, SlotError<String>> = worker_slot.run(
            "op-load",
            RESERVATION,
            move |token| {
                gate.announce_and_block();
                assert!(token.is_cancelled());
                Ok(ReentrantRuntime {
                    slot: runtime_slot.clone(),
                    drops: worker_drops.clone(),
                })
            },
            |_, _| Ok(7),
        );
        result
    });
    probe.wait_entered();
    assert_eq!(slot.cancel("op-load"), Ok(true));
    probe.release();
    assert_eq!(worker.join_bounded(), Err(SlotError::Cancelled));
    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "cancelled load must be dropped, not cached"
    );
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Absent);
    assert!(!slot.status().operation_active);
}

/// A late cancellation discards the caller result after `work` returned; the
/// result's reentrant destructor must run outside the metadata lock so the
/// operation completes (bounded) instead of deadlocking.
#[test]
fn late_cancel_drops_caller_result_outside_the_lock() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let warm: Result<usize, SlotError<String>> = slot.run(
        "op-warm",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(warm, Ok("verified".len()));
    let result_drops = Arc::new(AtomicUsize::new(0));
    let (gate, probe) = Gate::pair();
    let result_slot = slot.clone();
    let worker_result_drops = result_drops.clone();
    let worker = spawn_bounded(move || {
        let dropped_slot = result_slot.clone();
        let result: Result<ReentrantResult, SlotError<String>> = result_slot.run(
            "op-late",
            RESERVATION,
            ok_loader(counters, drops),
            move |_, _| {
                gate.announce_and_block();
                Ok(ReentrantResult {
                    slot: dropped_slot.clone(),
                    drops: worker_result_drops.clone(),
                })
            },
        );
        result
    });
    probe.wait_entered();
    assert_eq!(slot.cancel("op-late"), Ok(true));
    probe.release();
    assert!(
        matches!(worker.join_bounded(), Err(SlotError::Cancelled)),
        "late cancellation must discard the caller result"
    );
    assert_eq!(result_drops.load(Ordering::SeqCst), 1);
    // The load itself completed and published, so the cache stays reusable.
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Ready);
    assert!(!slot.status().operation_active);
}

/// Invariant under concurrency: for one operation, an accepted cancel
/// (`Ok(true)`) and an `Ok` result are mutually exclusive. The success
/// decision and the removal of the active entry are linearized under the
/// same lock as `cancel`, so whoever wins the lock last defines the outcome.
#[test]
fn cancel_and_completion_agree_under_concurrency() {
    for iteration in 0..200 {
        let slot = new_slot();
        let id = format!("op-conc-{iteration}");
        let worker = spawn_bounded({
            let slot = slot.clone();
            let id = id.clone();
            move || {
                let result: Result<usize, SlotError<String>> = slot.run(
                    id.as_str(),
                    RESERVATION,
                    |_| {
                        Ok(FakeRuntime {
                            label: "verified",
                            drops: Arc::new(AtomicUsize::new(0)),
                        })
                    },
                    |runtime, _| Ok(runtime.label.len()),
                );
                result
            }
        });
        // Cancel-spam from this thread while the tiny operation runs; then
        // collect the result through the bounded channel.
        let mut accepted_cancel = false;
        let result = loop {
            match worker.result.recv_timeout(Duration::ZERO) {
                Ok(result) => break result,
                Err(RecvTimeoutError::Timeout) => {
                    if slot.cancel(id.as_str()).unwrap_or(false) {
                        accepted_cancel = true;
                    }
                    thread::yield_now();
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("worker thread disconnected without a result")
                }
            }
        };
        worker.handle.join().expect("worker must not panic");
        assert!(
            !(accepted_cancel && result.is_ok()),
            "accepted cancel and successful result both observed for {id}"
        );
        // After completion no cancel can be accepted anymore.
        assert_eq!(slot.cancel(id.as_str()), Ok(false));
    }
}

/// Linearization, transition level: once `cancel` has won the metadata lock
/// and flagged the operation, `complete` under the same lock must observe the
/// flag and reject the result; after completion no `cancel` is accepted.
#[test]
fn completion_is_linearized_with_cancel_under_one_lock() {
    let slot = new_slot();
    let shared = &slot.shared;
    {
        let mut state = shared.lock_state();
        state.next_seq = 41;
        state.active = Some(ActiveOperation {
            seq: 40,
            operation_id: "op".to_string(),
            cancelled: Arc::new(AtomicBool::new(false)),
        });
    }
    // Cancel wins the lock first.
    assert_eq!(slot.cancel("op"), Ok(true));
    assert!(
        shared.complete(40),
        "accepted cancel before completion must discard the result"
    );
    assert!(!slot.status().operation_active);
    // Completion already happened: a later cancel must miss.
    assert_eq!(slot.cancel("op"), Ok(false));
}

/// Linearization, publish level: a cancel accepted before the publish must
/// make the under-one-lock publish refuse; an un-cancelled operation
/// publishes exactly once and a stale seq never publishes.
#[test]
fn publish_is_linearized_with_cancel_under_one_lock() {
    let slot = new_slot();
    let shared = &slot.shared;
    let runtime = Arc::new(FakeRuntime {
        label: "verified",
        drops: Arc::new(AtomicUsize::new(0)),
    });
    {
        let mut state = shared.lock_state();
        state.next_seq = 11;
        state.active = Some(ActiveOperation {
            seq: 9,
            operation_id: "op".to_string(),
            cancelled: Arc::new(AtomicBool::new(false)),
        });
    }
    assert_eq!(slot.cancel("op"), Ok(true));
    assert!(
        !shared.try_publish(9, &runtime),
        "cancel accepted before the publish must prevent caching"
    );
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Absent);
    // A fresh un-cancelled operation publishes under the same lock.
    {
        let mut state = shared.lock_state();
        state.active = Some(ActiveOperation {
            seq: 10,
            operation_id: "op".to_string(),
            cancelled: Arc::new(AtomicBool::new(false)),
        });
    }
    assert!(shared.try_publish(10, &runtime));
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Ready);
    // A stale seq from an earlier operation can never publish.
    assert!(!shared.try_publish(9, &runtime));
}

/// A stale completion must never clear a newer operation, even one reusing
/// the same id string: the guard matches the unique per-admission seq, not
/// the id. Here operation "dup" (seq 0) completes and its guard runs late,
/// after a new "dup" (seq 1) has been admitted and holds the slot.
#[test]
fn stale_completion_cannot_clear_a_same_id_operation() {
    let slot = new_slot();
    let shared = &slot.shared;
    {
        let mut state = shared.lock_state();
        state.next_seq = 1;
        state.active = Some(ActiveOperation {
            seq: 0,
            operation_id: "dup".to_string(),
            cancelled: Arc::new(AtomicBool::new(false)),
        });
    }
    assert!(!shared.complete(0));
    let (gate, probe) = Gate::pair();
    let worker_slot = slot.clone();
    let worker = spawn_bounded(move || {
        let result: Result<usize, SlotError<String>> = worker_slot.run(
            "dup",
            RESERVATION,
            |_| {
                Ok(FakeRuntime {
                    label: "verified",
                    drops: Arc::new(AtomicUsize::new(0)),
                })
            },
            move |runtime, _| {
                gate.announce_and_block();
                Ok(runtime.label.len())
            },
        );
        result
    });
    probe.wait_entered();
    // The stale seq-0 guard finally runs; the new same-id operation survives.
    drop(OperationGuard { shared, seq: 0 });
    assert!(
        slot.status().operation_active,
        "stale completion must not clear the new same-id operation"
    );
    // The new operation is still the one `cancel` addresses and it still
    // holds the slot.
    assert_eq!(slot.cancel("dup"), Ok(true));
    let contender = spawn_bounded({
        let slot = slot.clone();
        move || {
            let result: Result<usize, SlotError<String>> = slot.run(
                "dup",
                RESERVATION,
                |_| {
                    Ok(FakeRuntime {
                        label: "verified",
                        drops: Arc::new(AtomicUsize::new(0)),
                    })
                },
                |_, _| Ok(0),
            );
            result
        }
    });
    assert_eq!(contender.join_bounded(), Err(SlotError::Busy));
    probe.release();
    assert_eq!(worker.join_bounded(), Err(SlotError::Cancelled));
}

/// Two cloned handles describe one logical slot: cancelling through handle A
/// is observed by the operation running on handle B.
#[test]
fn cloned_handles_share_one_cached_runtime_and_cancel_state() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot_a = new_slot();
    let slot_b = slot_a.clone();
    let warm: Result<usize, SlotError<String>> = slot_a.run(
        "op-warm",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(warm, Ok("verified".len()));
    assert_eq!(slot_b.status().cached_runtime, CachedRuntime::Ready);
    let (gate, probe) = Gate::pair();
    let worker_slot = slot_b.clone();
    let worker = spawn_bounded(move || {
        let result: Result<usize, SlotError<String>> = worker_slot.run(
            "op-shared",
            RESERVATION,
            ok_loader(counters, drops),
            move |runtime, _| {
                gate.announce_and_block();
                Ok(runtime.label.len())
            },
        );
        result
    });
    probe.wait_entered();
    assert_eq!(slot_a.cancel("op-shared"), Ok(true));
    probe.release();
    assert_eq!(worker.join_bounded(), Err(SlotError::Cancelled));
    assert_eq!(slot_a.unload(), Ok(true));
    assert_eq!(slot_b.status().cached_runtime, CachedRuntime::Absent);
}

/// Operation ids are opaque: never trimmed or normalized. Surrounding
/// whitespace is rejected outright, and the ORIGINAL byte length is bound so
/// padding cannot smuggle an over-limit id past the check.
#[test]
fn operation_ids_are_opaque_and_never_trimmed() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    for padded in [
        "  op-padded  ",
        " op-padded",
        "op-padded ",
        "\u{00a0}op-padded",
    ] {
        let result: Result<usize, SlotError<String>> = slot.run(
            padded,
            RESERVATION,
            ok_loader(counters.clone(), drops.clone()),
            ok_work(counters.clone()),
        );
        assert_eq!(result, Err(SlotError::InvalidOperationId), "{padded:?}");
    }
    // An id whose trimmed form would fit must still fail: the bound applies
    // to the original bytes.
    let padded_over = format!(" {} ", "z".repeat(MAX_OPERATION_ID_BYTES));
    assert!(padded_over.len() > MAX_OPERATION_ID_BYTES);
    let over: Result<usize, SlotError<String>> = slot.run(
        padded_over.as_str(),
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(over, Err(SlotError::InvalidOperationId));
    assert_eq!(
        slot.cancel("  op-padded  "),
        Err(SlotError::InvalidOperationId)
    );
    // Exactly at the original bound is admitted.
    let exact = "y".repeat(MAX_OPERATION_ID_BYTES);
    let admitted: Result<usize, SlotError<String>> = slot.run(
        exact.as_str(),
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(admitted, Ok("verified".len()));
    assert_eq!(counters.load_count(), 1);
    assert_eq!(counters.work_count(), 1);
}

#[test]
fn slot_and_token_are_send_and_sync_for_shared_handles() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RuntimeSlot<FakeRuntime>>();
    assert_send_sync::<CancelToken>();
    assert_send_sync::<SlotStatus>();
}

// ---------------------------------------------------------------------------
// Exclusive maintenance operation (`maintain`): same single-operation gate,
// guard and linearized cancellation as `run`, but no runtime is loaded,
// required, published or removed. The fake runtime/drop counters stand in for
// the host-owned pinned-package work; no E5 data, no filesystem, no network.

/// Maintenance callback that succeeds with a fixed marker value.
fn maintain_work(
    counters: Arc<CallbackCounters>,
) -> impl Fn(&CancelToken) -> Result<usize, String> {
    move |_| {
        counters.works.fetch_add(1, Ordering::SeqCst);
        Ok(41)
    }
}

#[test]
fn maintain_runs_host_work_without_touching_the_cache() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let result: Result<usize, SlotError<String>> =
        slot.maintain("op-maintain", maintain_work(counters.clone()));
    assert_eq!(result, Ok(41));
    assert_eq!(counters.work_count(), 1);
    assert_eq!(counters.load_count(), 0);
    // Maintenance neither loads nor drops a runtime.
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(
        slot.status(),
        SlotStatus {
            cached_runtime: CachedRuntime::Absent,
            operation_active: false,
            resident_reservation_bytes: RESERVATION,
        }
    );
}

#[test]
fn maintain_rejects_invalid_operation_ids_before_callback() {
    let counters = Arc::new(CallbackCounters::new());
    let slot = new_slot();
    let long_id = "m".repeat(MAX_OPERATION_ID_BYTES + 1);
    for invalid in ["", "   ", long_id.as_str(), "op\0x", " op-padded"] {
        let result: Result<usize, SlotError<String>> =
            slot.maintain(invalid, maintain_work(counters.clone()));
        assert_eq!(result, Err(SlotError::InvalidOperationId), "{invalid:?}");
    }
    assert_eq!(counters.work_count(), 0);
    assert!(!slot.status().operation_active);
}

#[test]
fn maintain_leaves_cached_runtime_ready_and_reusable() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let warm: Result<usize, SlotError<String>> = slot.run(
        "op-warm",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(warm, Ok("verified".len()));
    // While maintenance runs, RAM readiness is reported unchanged.
    let (gate, probe) = Gate::pair();
    let worker = spawn_bounded({
        let slot = slot.clone();
        let counters = counters.clone();
        move || {
            let result: Result<usize, SlotError<String>> =
                slot.maintain("op-maintain", move |token| {
                    gate.announce_and_block();
                    assert!(!token.is_cancelled());
                    counters.works.fetch_add(1, Ordering::SeqCst);
                    Ok(41)
                });
            result
        }
    });
    probe.wait_entered();
    let status = slot.status();
    assert!(status.operation_active);
    assert_eq!(
        status.cached_runtime,
        CachedRuntime::Ready,
        "maintenance must not remove or publish the cached runtime"
    );
    probe.release();
    assert_eq!(worker.join_bounded(), Ok(41));
    // The cached runtime is reused unchanged: the loader still ran once.
    let after: Result<usize, SlotError<String>> = slot.run(
        "op-after",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(after, Ok("verified".len()));
    assert_eq!(counters.load_count(), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
}

#[test]
fn run_holding_slot_fails_maintain_busy_before_its_callback() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let (gate, probe) = Gate::pair();
    let run_slot = slot.clone();
    let run_counters = counters.clone();
    let run_drops = drops.clone();
    let running = spawn_bounded(move || {
        let result: Result<usize, SlotError<String>> = run_slot.run(
            "op-run",
            RESERVATION,
            move |_| {
                run_counters.loads.fetch_add(1, Ordering::SeqCst);
                Ok(FakeRuntime {
                    label: "verified",
                    drops: run_drops,
                })
            },
            move |runtime, _| {
                gate.announce_and_block();
                Ok(runtime.label.len())
            },
        );
        result
    });
    probe.wait_entered();
    // The maintenance contender fails Busy before its callback runs; nothing
    // queues behind the active runtime operation.
    let maintain_counters = Arc::new(CallbackCounters::new());
    let contender = spawn_bounded({
        let slot = slot.clone();
        let maintain_counters = maintain_counters.clone();
        move || {
            let result: Result<usize, SlotError<String>> =
                slot.maintain("op-maintain", maintain_work(maintain_counters));
            result
        }
    });
    assert_eq!(
        contender.join_bounded(),
        Err(SlotError::Busy),
        "maintenance must share the run admission gate"
    );
    assert_eq!(maintain_counters.work_count(), 0);
    probe.release();
    assert_eq!(running.join_bounded(), Ok("verified".len()));
    // Once the slot is free, maintenance is admitted.
    let after: Result<usize, SlotError<String>> =
        slot.maintain("op-maintain", maintain_work(maintain_counters.clone()));
    assert_eq!(after, Ok(41));
    assert_eq!(maintain_counters.work_count(), 1);
}

#[test]
fn maintain_holding_slot_fails_run_busy_before_any_loader() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let (gate, probe) = Gate::pair();
    let maintain_slot = slot.clone();
    let maintaining = spawn_bounded(move || {
        let result: Result<usize, SlotError<String>> =
            maintain_slot.maintain("op-maintain", move |_| {
                gate.announce_and_block();
                Ok(41)
            });
        result
    });
    probe.wait_entered();
    // Runtime work must not be admitted while maintenance holds the gate;
    // the loader is never invoked.
    let contender = spawn_bounded({
        let slot = slot.clone();
        let counters = counters.clone();
        let drops = drops.clone();
        move || {
            let result: Result<usize, SlotError<String>> = slot.run(
                "op-run",
                RESERVATION,
                ok_loader(counters, drops),
                ok_work(Arc::new(CallbackCounters::new())),
            );
            result
        }
    });
    assert_eq!(
        contender.join_bounded(),
        Err(SlotError::Busy),
        "runtime work must wait for the active maintenance operation"
    );
    assert_eq!(counters.load_count(), 0);
    probe.release();
    assert_eq!(maintaining.join_bounded(), Ok(41));
    let after: Result<usize, SlotError<String>> = slot.run(
        "op-run",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(after, Ok("verified".len()));
    assert_eq!(counters.load_count(), 1);
}

#[test]
fn unload_fails_busy_during_maintenance() {
    let slot = new_slot();
    let warm: Result<usize, SlotError<String>> = slot.run(
        "op-warm",
        RESERVATION,
        |_| {
            Ok(FakeRuntime {
                label: "verified",
                drops: Arc::new(AtomicUsize::new(0)),
            })
        },
        |runtime, _| Ok(runtime.label.len()),
    );
    assert_eq!(warm, Ok("verified".len()));
    let (gate, probe) = Gate::pair();
    let worker_slot = slot.clone();
    let worker = spawn_bounded(move || {
        let result: Result<usize, SlotError<String>> =
            worker_slot.maintain("op-maintain", move |_| {
                gate.announce_and_block();
                Ok(41)
            });
        result
    });
    probe.wait_entered();
    assert_eq!(
        slot.unload(),
        Err(SlotError::Busy),
        "unload must fail while maintenance is active"
    );
    probe.release();
    assert_eq!(worker.join_bounded(), Ok(41));
    // Maintenance removed nothing; the idle unload drops the cached runtime.
    assert_eq!(slot.unload(), Ok(true));
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Absent);
}

#[test]
fn cancel_during_maintenance_discards_result_and_holds_busy_until_exit() {
    let counters = Arc::new(CallbackCounters::new());
    let slot = new_slot();
    let (gate, probe) = Gate::pair();
    let worker_slot = slot.clone();
    let worker = spawn_bounded(move || {
        let result: Result<usize, SlotError<String>> =
            worker_slot.maintain("op-maintain", move |token| {
                gate.announce_and_block();
                Ok(if token.is_cancelled() { 0 } else { 41 })
            });
        result
    });
    probe.wait_entered();
    // Cancellation is requested through a different cloned handle and affects
    // only the exactly matching active operation id.
    assert_eq!(slot.cancel("op-maintain"), Ok(true));
    assert!(
        slot.status().operation_active,
        "cooperative cancel keeps the slot busy until the callback returns"
    );
    // An accepted cancel is not an admission: runtime work still fails Busy.
    let contender: Result<usize, SlotError<String>> = slot.run(
        "op-run",
        RESERVATION,
        ok_loader(counters.clone(), Arc::new(AtomicUsize::new(0))),
        ok_work(counters.clone()),
    );
    assert_eq!(contender, Err(SlotError::Busy));
    assert_eq!(counters.load_count(), 0);
    probe.release();
    assert_eq!(
        worker.join_bounded(),
        Err(SlotError::Cancelled),
        "a cancelled maintenance result never surfaces as success"
    );
    assert!(!slot.status().operation_active);
    assert_eq!(slot.cancel("op-maintain"), Ok(false));
    // The slot is freed and usable again.
    let after: Result<usize, SlotError<String>> =
        slot.maintain("op-after", maintain_work(Arc::new(CallbackCounters::new())));
    assert_eq!(after, Ok(41));
}

#[test]
fn stale_id_cannot_cancel_active_maintenance() {
    let slot = new_slot();
    let (gate, probe) = Gate::pair();
    let worker_slot = slot.clone();
    let worker = spawn_bounded(move || {
        let result: Result<usize, SlotError<String>> =
            worker_slot.maintain("op-current", move |_| {
                gate.announce_and_block();
                Ok(41)
            });
        result
    });
    probe.wait_entered();
    assert_eq!(slot.cancel("op-stale"), Ok(false));
    probe.release();
    assert_eq!(worker.join_bounded(), Ok(41));
    assert!(!slot.status().operation_active);
}

#[test]
fn failed_maintenance_preserves_typed_payload_and_frees_slot() {
    let counters = Arc::new(CallbackCounters::new());
    let slot = new_slot();
    let failed: Result<usize, SlotError<String>> =
        slot.maintain("op-fail", |_| Err("package checksum mismatch".to_string()));
    assert_eq!(
        failed,
        Err(SlotError::WorkFailed(
            "package checksum mismatch".to_string()
        )),
        "host error payload must pass through verbatim"
    );
    assert!(!slot.status().operation_active);
    let retry: Result<usize, SlotError<String>> =
        slot.maintain("op-retry", maintain_work(counters.clone()));
    assert_eq!(retry, Ok(41));
    assert_eq!(counters.work_count(), 1);
}

#[test]
fn maintain_callback_can_query_status_and_cancel_without_deadlock() {
    let counters = Arc::new(CallbackCounters::new());
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = new_slot();
    let warm: Result<usize, SlotError<String>> = slot.run(
        "op-warm",
        RESERVATION,
        ok_loader(counters.clone(), drops.clone()),
        ok_work(counters.clone()),
    );
    assert_eq!(warm, Ok("verified".len()));
    let result: Result<usize, SlotError<String>> = slot.maintain("op-self", |token| {
        // Re-entering the slot from inside maintenance must be lock-free.
        assert!(!token.is_cancelled());
        let status = slot.status();
        assert!(status.operation_active);
        assert_eq!(
            status.cached_runtime,
            CachedRuntime::Ready,
            "RAM readiness is unchanged while maintenance runs"
        );
        assert_eq!(slot.cancel("op-self"), Ok(true));
        assert!(token.is_cancelled());
        Ok(41)
    });
    // The callback finished after requesting its own cancellation, so the
    // late cancellation discards the value.
    assert_eq!(result, Err(SlotError::Cancelled));
    assert_eq!(slot.status().cached_runtime, CachedRuntime::Ready);
}

#[test]
fn panic_in_maintenance_clears_busy_and_never_wedges_the_slot() {
    let counters = Arc::new(CallbackCounters::new());
    let slot = new_slot();
    let panicking = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<usize, SlotError<String>> = slot
            .maintain("op-panic", |_| -> Result<usize, String> {
                panic!("maintenance blew up")
            });
    }));
    assert!(panicking.is_err());
    let status = slot.status();
    assert!(
        !status.operation_active,
        "RAII cleanup must clear the busy flag"
    );
    // The shared guard is seq-matched, so the slot stays usable and runtime
    // operations are admitted again.
    let recovered: Result<usize, SlotError<String>> = slot.run(
        "op-recover",
        RESERVATION,
        ok_loader(counters.clone(), Arc::new(AtomicUsize::new(0))),
        ok_work(counters.clone()),
    );
    assert_eq!(recovered, Ok("verified".len()));
}

/// Invariant under concurrency for maintenance: an accepted cancel
/// (`Ok(true)`) and an `Ok` result are mutually exclusive. The success
/// decision and the removal of the active entry are linearized under the
/// same lock as `cancel`, so whoever wins the lock last defines the outcome.
#[test]
fn cancel_and_maintenance_completion_agree_under_concurrency() {
    for iteration in 0..200 {
        let slot = new_slot();
        let id = format!("op-maintain-{iteration}");
        let worker = spawn_bounded({
            let slot = slot.clone();
            let id = id.clone();
            move || {
                let result: Result<usize, SlotError<String>> =
                    slot.maintain(id.as_str(), |_| Ok(41));
                result
            }
        });
        let mut accepted_cancel = false;
        let result = loop {
            match worker.result.recv_timeout(Duration::ZERO) {
                Ok(result) => break result,
                Err(RecvTimeoutError::Timeout) => {
                    if slot.cancel(id.as_str()).unwrap_or(false) {
                        accepted_cancel = true;
                    }
                    thread::yield_now();
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("worker thread disconnected without a result")
                }
            }
        };
        worker.handle.join().expect("worker must not panic");
        assert!(
            !(accepted_cancel && result.is_ok()),
            "accepted cancel and successful result both observed for {id}"
        );
        assert_eq!(slot.cancel(id.as_str()), Ok(false));
    }
}
