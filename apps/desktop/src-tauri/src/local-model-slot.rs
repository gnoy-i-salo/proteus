//! Native runtime-resource slot for one resident runtime (e.g. `E5Runtime`).
//!
//! `RuntimeSlot<T>` owns at most one cached runtime and admits at most one
//! active operation across all cloned handles. The generic `T` plus the
//! caller-supplied `load`/`work` closures exist solely so this module can
//! own and reuse one verified heavyweight runtime deterministically under
//! std-only tests; this is not a DI framework.
//!
//! The slot never authenticates or authorizes text, fetches models, verifies
//! files or touches the network; [`SlotStatus`] reports in-memory
//! cached-runtime readiness only, never disk-cache/package readiness, and
//! callers never choose source scope or cache paths through this API.
//! `resident_reservation_bytes` is fixed at construction; a smaller
//! `max_resident_bytes` fails before any callback, also on reuse. This
//! declares exclusivity, not a measured OS RSS bound.
//!
//! Operation ids are opaque: never trimmed or normalized; surrounding
//! whitespace is rejected outright and the ORIGINAL byte length is bounded.
//! Cancellation is cooperative via [`CancelToken`]: the slot stays busy until
//! the callback returns, a cancelled result never surfaces as success, and
//! each publish/completion decision is linearized with `cancel` under one
//! lock. Heavyweight values are never dropped under the metadata lock; their
//! destructors may reenter `status`. Callback errors pass through verbatim in
//! [`SlotError::LoadFailed`] / [`SlotError::WorkFailed`], never
//! sentinel-classified. Exclusive maintenance that needs the same
//! single-operation gate but no runtime (pinned-package inspection/download)
//! runs through [`RuntimeSlot::maintain`]; it shares the admission, guard and
//! linearized cancellation of `run` and never loads, publishes or removes the
//! cached runtime.
use std::convert::Infallible;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Maximum accepted ORIGINAL byte length of an operation id.
pub const MAX_OPERATION_ID_BYTES: usize = 128;
/// Upper bound for reservation and budget values so absurdly large byte
/// counts are rejected before any callback runs.
pub const MAX_RESIDENT_BYTES: u64 = 9_007_199_254_740_991;

/// Typed failure of a slot operation. `E` is the host callback error payload,
/// preserved exactly as the callback produced it.
#[derive(Debug, PartialEq, Eq)]
pub enum SlotError<E = Infallible> {
    /// Another operation already holds the slot; work is never queued.
    Busy,
    /// Operation id is empty, exceeds [`MAX_OPERATION_ID_BYTES`], contains
    /// control characters or has surrounding whitespace.
    InvalidOperationId,
    /// Resident budget is zero or exceeds [`MAX_RESIDENT_BYTES`].
    InvalidBudget,
    /// Resident budget is well-formed but below the slot's fixed reservation.
    ResidentReservation,
    /// The exact active operation was cancelled and its result discarded.
    Cancelled,
    /// Host load callback failed; payload preserved verbatim.
    LoadFailed(E),
    /// Host work callback failed; payload preserved verbatim.
    WorkFailed(E),
}
impl<E: fmt::Display> fmt::Display for SlotError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => write!(formatter, "slot is busy with another operation"),
            Self::InvalidOperationId => write!(
                formatter,
                "operation id must be 1..={MAX_OPERATION_ID_BYTES} bytes, free of control characters and surrounding whitespace"
            ),
            Self::InvalidBudget => write!(
                formatter,
                "resident budget must be between 1 and {MAX_RESIDENT_BYTES} bytes"
            ),
            Self::ResidentReservation => {
                write!(formatter, "resident budget is below the slot's fixed reservation")
            }
            Self::Cancelled => write!(formatter, "operation was cancelled"),
            Self::LoadFailed(payload) => write!(formatter, "load callback failed: {payload}"),
            Self::WorkFailed(payload) => write!(formatter, "work callback failed: {payload}"),
        }
    }
}
impl<E: fmt::Display + fmt::Debug> std::error::Error for SlotError<E> {}

/// Presence of the in-memory cached runtime; deliberately not a disk-cache or
/// package-readiness state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CachedRuntime {
    /// No runtime is cached; the next `run` must invoke the loader.
    Absent,
    /// A runtime is cached and reusable; the next `run` skips the loader.
    Ready,
}

/// Read-only snapshot of the slot. `cached_runtime` tracks in-memory runtime
/// readiness only; on-disk package readiness is owned elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotStatus {
    pub cached_runtime: CachedRuntime,
    pub operation_active: bool,
    pub resident_reservation_bytes: u64,
}

/// Cooperative cancellation flag for exactly one in-flight operation. Cheap to
/// clone; observing `is_cancelled` never blocks.
#[derive(Clone, Debug)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
}
impl CancelToken {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

struct ActiveOperation {
    /// Unique per admission: a stale completion can never touch a newer
    /// operation, even one reusing the same id string.
    seq: u64,
    operation_id: String,
    cancelled: Arc<AtomicBool>,
}

/// One admitted operation: the unique per-admission seq, the operation-local
/// cancellation flag and the cached runtime observed at admission. The cached
/// runtime clone is taken under the same lock hold that installs the active
/// operation, so no idle `unload` can slip in between.
struct Admission<T> {
    seq: u64,
    cancelled: Arc<AtomicBool>,
    cached_runtime: Option<Arc<T>>,
}

struct State<T> {
    cached_runtime: Option<Arc<T>>,
    active: Option<ActiveOperation>,
    next_seq: u64,
}

struct Shared<T> {
    resident_reservation_bytes: u64,
    state: Mutex<State<T>>,
}
impl<T> Shared<T> {
    /// Never held across a callback, so a callback panic cannot poison the
    /// mutex; recover anyway so the slot can never wedge.
    fn lock_state(&self) -> MutexGuard<'_, State<T>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Shared single-operation admission for `run` and `maintain`: takes the
    /// gate or fails `Busy` before any callback, installing a fresh
    /// per-admission seq. There is no second gate: maintenance and runtime
    /// work exclude each other through this one active slot. The caller has
    /// already validated `operation_id` and applies any further checks
    /// (resident budget for `run`) before calling.
    fn admit<E>(&self, operation_id: &str) -> Result<Admission<T>, SlotError<E>> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut state = self.lock_state();
        if state.active.is_some() {
            return Err(SlotError::Busy);
        }
        let seq = state.next_seq;
        state.next_seq += 1;
        let cached_runtime = state.cached_runtime.clone();
        state.active = Some(ActiveOperation {
            seq,
            operation_id: operation_id.to_string(),
            cancelled: Arc::clone(&cancelled),
        });
        Ok(Admission {
            seq,
            cancelled,
            cached_runtime,
        })
    }

    /// Publishes `runtime` iff operation `seq` is still the active,
    /// un-cancelled operation. Check-and-publish runs under the same lock as
    /// `cancel`, so cancellation cannot land between check and assignment.
    fn try_publish(&self, seq: u64, runtime: &Arc<T>) -> bool {
        let mut state = self.lock_state();
        let publish = match state.active.as_ref() {
            Some(active) if active.seq == seq => !active.cancelled.load(Ordering::Acquire),
            _ => false,
        };
        if publish {
            state.cached_runtime = Some(Arc::clone(runtime));
        }
        publish
    }

    /// Removes the active operation `seq` and reports whether it was
    /// cancelled. Runs under the same lock as `cancel`, so whoever acquires
    /// it last defines the outcome. Fails closed if `seq` is not active.
    fn complete(&self, seq: u64) -> bool {
        let mut state = self.lock_state();
        match state.active.as_ref() {
            Some(active) if active.seq == seq => {
                let cancelled = active.cancelled.load(Ordering::Acquire);
                state.active = None;
                cancelled
            }
            _ => true,
        }
    }
}

/// Handle to one logical runtime slot. Cloning shares the same cache, busy
/// flag and reservation: at most one operation is active across all handles.
pub struct RuntimeSlot<T> {
    shared: Arc<Shared<T>>,
}
impl<T> Clone for RuntimeSlot<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}
impl<T> RuntimeSlot<T> {
    /// Creates a slot with a fixed resident reservation chosen by native
    /// construction; the value cannot be changed afterwards.
    pub fn new(resident_reservation_bytes: u64) -> Result<Self, SlotError> {
        if resident_reservation_bytes == 0 || resident_reservation_bytes > MAX_RESIDENT_BYTES {
            return Err(SlotError::InvalidBudget);
        }
        Ok(Self {
            shared: Arc::new(Shared {
                resident_reservation_bytes,
                state: Mutex::new(State {
                    cached_runtime: None,
                    active: None,
                    next_seq: 0,
                }),
            }),
        })
    }
    /// The fixed reservation declared at construction.
    pub fn resident_reservation_bytes(&self) -> u64 {
        self.shared.resident_reservation_bytes
    }
    /// Read-only snapshot of cached-runtime presence and active operation.
    pub fn status(&self) -> SlotStatus {
        let state = self.shared.lock_state();
        SlotStatus {
            cached_runtime: if state.cached_runtime.is_some() {
                CachedRuntime::Ready
            } else {
                CachedRuntime::Absent
            },
            operation_active: state.active.is_some(),
            resident_reservation_bytes: self.shared.resident_reservation_bytes,
        }
    }
    /// Runs one operation: admit, (load or reuse), work, return.
    ///
    /// Admission is shared with [`RuntimeSlot::maintain`]: both take the same
    /// single-operation gate, so at most one of them is active across all
    /// cloned handles. Admission order is fixed: validate the id, validate the
    /// budget, check the reservation, then fail `Busy` if another operation
    /// holds the slot. The cached runtime is reused without invoking `load`
    /// again. The mutex is released before any callback and re-acquired only
    /// for the small linearized transitions in [`Shared`].
    pub fn run<R, E, L, W>(
        &self,
        operation_id: &str,
        max_resident_bytes: u64,
        load: L,
        work: W,
    ) -> Result<R, SlotError<E>>
    where
        L: FnOnce(&CancelToken) -> Result<T, E>,
        W: FnOnce(&T, &CancelToken) -> Result<R, E>,
    {
        let operation_id = validate_operation_id(operation_id)?;
        validate_resident_budget(max_resident_bytes)?;
        if max_resident_bytes < self.shared.resident_reservation_bytes {
            return Err(SlotError::ResidentReservation);
        }
        let admission = self.shared.admit(operation_id)?;
        let _guard = OperationGuard {
            shared: &self.shared,
            seq: admission.seq,
        };
        let token = CancelToken {
            cancelled: admission.cancelled,
        };
        fail_if_cancelled(&token)?;
        let runtime = match admission.cached_runtime {
            Some(runtime) => runtime,
            None => {
                let loaded = load(&token).map_err(SlotError::LoadFailed)?;
                let runtime = Arc::new(loaded);
                if !self.shared.try_publish(admission.seq, &runtime) {
                    // Cancel won the lock before the publish: the runtime is
                    // dropped after the guard is gone (its destructor may
                    // reenter the slot), never cached, and `work` never runs.
                    return Err(SlotError::Cancelled);
                }
                runtime
            }
        };
        fail_if_cancelled(&token)?;
        let result = work(&runtime, &token).map_err(SlotError::WorkFailed)?;
        // Linearized completion: an accepted cancellation discards `result`
        // (dropped after the lock is released); otherwise the operation
        // returns and a later `cancel` observes no active entry.
        if self.shared.complete(admission.seq) {
            return Err(SlotError::Cancelled);
        }
        Ok(result)
    }
    /// Runs one exclusive maintenance operation that needs the slot's
    /// admission gate but no runtime (e.g. pinned-package inspection/download
    /// owned by the native host; the caller action is host-owned, never model
    /// code). Shares the exact same operation state, guard and linearized
    /// cancellation as [`RuntimeSlot::run`]: at most one of `run`/`maintain`
    /// is active across all cloned handles, contenders fail `Busy` before any
    /// callback, nothing queues, `cancel` addresses only the exactly matching
    /// active id, and a cancelled result never surfaces as success.
    ///
    /// Maintenance takes no resident-budget argument because it never loads a
    /// runtime; it neither requires, publishes nor removes the cached runtime,
    /// so the RAM readiness reported by [`RuntimeSlot::status`] is unchanged
    /// and [`RuntimeSlot::unload`] still fails `Busy` while maintenance is
    /// active. The state mutex is released before the callback and
    /// re-acquired only for the linearized completion.
    pub fn maintain<R, E, W>(&self, operation_id: &str, work: W) -> Result<R, SlotError<E>>
    where
        W: FnOnce(&CancelToken) -> Result<R, E>,
    {
        let operation_id = validate_operation_id(operation_id)?;
        let admission = self.shared.admit(operation_id)?;
        let _guard = OperationGuard {
            shared: &self.shared,
            seq: admission.seq,
        };
        let token = CancelToken {
            cancelled: admission.cancelled,
        };
        fail_if_cancelled(&token)?;
        let result = work(&token).map_err(SlotError::WorkFailed)?;
        // Same linearized completion as `run`: an accepted cancellation
        // discards `result` (dropped after the lock is released).
        if self.shared.complete(admission.seq) {
            return Err(SlotError::Cancelled);
        }
        Ok(result)
    }
    /// Flags cancellation for the exactly matching active operation id — of
    /// either `run` or `maintain` — and returns whether an operation was
    /// flagged (`Ok(false)` for well-formed ids that match nothing; invalid
    /// ids fail). Cooperative: the active callback keeps running, the slot
    /// stays busy until it returns, and an accepted cancel always rejects the
    /// operation's result.
    pub fn cancel(&self, operation_id: &str) -> Result<bool, SlotError> {
        let operation_id = validate_operation_id(operation_id)?;
        let state = self.shared.lock_state();
        match &state.active {
            Some(active) if active.operation_id == *operation_id => {
                active.cancelled.store(true, Ordering::Release);
                Ok(true)
            }
            _ => Ok(false),
        }
    }
    /// Drops the cached runtime, but only while no operation — runtime work or
    /// maintenance — is active; returns whether a runtime was dropped.
    pub fn unload(&self) -> Result<bool, SlotError> {
        let cached = {
            let mut state = self.shared.lock_state();
            if state.active.is_some() {
                return Err(SlotError::Busy);
            }
            state.cached_runtime.take()
        };
        // The last-handle drop may run a heavyweight destructor that reenters
        // the slot (e.g. `status`), so it must happen after the mutex guard
        // on `state` is released.
        let had_cached = cached.is_some();
        drop(cached);
        Ok(had_cached)
    }
}

/// Clears the active operation on scope exit (callback failure or panic), but
/// only when it still carries this guard's unique seq: a stale drop can never
/// clear another operation's state, even one reusing the same id string.
struct OperationGuard<'a, T> {
    shared: &'a Shared<T>,
    seq: u64,
}
impl<T> Drop for OperationGuard<'_, T> {
    fn drop(&mut self) {
        let mut state = self.shared.lock_state();
        if state
            .active
            .as_ref()
            .is_some_and(|active| active.seq == self.seq)
        {
            state.active = None;
        }
    }
}

/// Ids are opaque: never trimmed or canonicalized. Surrounding whitespace and
/// control characters are rejected outright, and the ORIGINAL byte length is
/// bounded so padding cannot smuggle an over-limit id past the check.
fn validate_operation_id<E>(operation_id: &str) -> Result<&str, SlotError<E>> {
    let valid = !operation_id.is_empty()
        && operation_id.len() <= MAX_OPERATION_ID_BYTES
        && operation_id.trim() == operation_id
        && !operation_id.chars().any(char::is_control);
    if !valid {
        return Err(SlotError::InvalidOperationId);
    }
    Ok(operation_id)
}

fn validate_resident_budget<E>(max_resident_bytes: u64) -> Result<(), SlotError<E>> {
    if max_resident_bytes == 0 || max_resident_bytes > MAX_RESIDENT_BYTES {
        return Err(SlotError::InvalidBudget);
    }
    Ok(())
}

fn fail_if_cancelled<E>(token: &CancelToken) -> Result<(), SlotError<E>> {
    if token.is_cancelled() {
        return Err(SlotError::Cancelled);
    }
    Ok(())
}

#[cfg(test)]
#[path = "local-model-slot-tests.rs"]
mod tests;
