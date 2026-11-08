//! Host-side cumulative budget ledger for native model-call attempts.
//!
//! `ModelCallLedger` tracks one bounded run of model attempts: every attempt
//! counts against the cumulative call and input-byte budgets, including
//! format-repair attempts, independent of retrieval accounting. It is the
//! native accounting half behind the shared `ModelCallBudget` /
//! `ModelContextLease` contracts: the host chooses the limits and the measured
//! `inputBytes` up front and the ledger only admits, charges and redeems
//! reservations. Model actions never supply or expand budgets through it.
//!
//! This is a bookkeeping ledger, not an authorization token or service:
//! - A reservation receipt is not authority. The internal sequence is a local
//!   correlation number, not a security credential; the host registry maps its
//!   own opaque lease IDs to sequences and validates host identity, source
//!   scope, persona manifest and input hash separately.
//! - Charging happens at reservation time: a successful `reserve` immediately
//!   consumes one call and all of the declared bytes. Nothing is ever
//!   refunded — unused, cancelled or failed downstream work keeps its charge.
//! - `consume` redeems a reservation exactly once. Both `reserve` and
//!   `consume` consult the checked clock twice: an initial reading validates
//!   the admission, then the state change happens (charge/store for
//!   `reserve`, mark-consumed for `consume`), and a final reading runs before
//!   any receipt is returned. A revocation, expiry or backwards move observed
//!   during the attempt can therefore never hand back a lease; work already
//!   charged keeps its charge and the ledger closes permanently.
//! - A failed clock/grant check, a negative reading, a backwards move or an
//!   expired (exclusive) deadline permanently closes the ledger; the original
//!   reason is preserved exactly, never sentinel-classified. Explicit `close`
//!   is idempotent and keeps the first reason.
//! - Resource limit rejections are explicit and happen before any clock read
//!   or charge; all arithmetic on caller-supplied magnitudes is checked.
//! - Records are bounded by [`MAX_LEDGER_CALLS`]: at most 1000 small records
//!   per ledger, so there is no unbounded growth and no huge allocation path.
//! - The ledger performs no network, CLI or filesystem work; unused
//!   reservations simply expire together with the ledger.
//! - One mutable owner serializes all operations (`&mut self`); no mutex or
//!   async is used here — the host session/lease registry synchronizes later.
//!
//! Deliberate limitations:
//! - The budget validates shapes and limits only. It does not validate the
//!   source scope, persona manifest, model hash or any external grant; those
//!   stay with the host-side owners named above.
//! - Time comes exclusively from the caller-supplied checked callback; the
//!   ledger never reads a wall clock itself.
//! - Monotonicity and the exclusive deadline are enforced against the last
//!   successfully observed time; a host clock that jumps backwards closes the
//!   ledger rather than being smoothed.
use std::fmt;

/// Upper bound for the ledger call budget; also bounds stored records.
pub const MAX_LEDGER_CALLS: u32 = 1000;
/// Upper bound for the cumulative input-byte budget.
pub const MAX_LEDGER_INPUT_BYTES: u64 = 64 * 1024 * 1024;
/// Upper bound for a single reservation's measured input bytes.
pub const MAX_RESERVATION_INPUT_BYTES: u64 = 4 * 1024 * 1024;
/// Upper bound for the ledger duration budget in milliseconds.
pub const MAX_LEDGER_DURATION_MS: i64 = 3_600_000;
/// Maximum accepted ORIGINAL byte length of a caller id.
pub const MAX_CALLER_ID_BYTES: usize = 128;

/// Typed failure of a ledger operation.
#[derive(Debug, PartialEq, Eq)]
pub enum BudgetError {
    /// Constructor limits are outside the accepted ranges.
    InvalidLimits,
    /// Constructor start wall clock is negative.
    InvalidStartTime,
    /// The ledger is permanently closed; the reason is preserved exactly.
    Closed(String),
    /// Caller id is empty, over-long, control-padded or surrounded by
    /// whitespace.
    InvalidCallerId,
    /// A reservation with this caller id already exists in the ledger.
    DuplicateCaller,
    /// Reservation input bytes are outside `1..=MAX_RESERVATION_INPUT_BYTES`.
    InvalidInputBytes,
    /// Reservation format attempt is not `1` or `2`.
    InvalidFormatAttempt,
    /// The reservation would exceed the cumulative call budget.
    CallLimitExceeded,
    /// The reservation would exceed the cumulative input-byte budget.
    InputLimitExceeded,
    /// No reservation carries this sequence.
    UnknownSequence,
    /// This reservation was already redeemed.
    AlreadyConsumed,
    /// Consumed expectations do not match the reservation metadata.
    ReservationMismatch,
}
impl fmt::Display for BudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimits => write!(formatter, "ledger limits out of range"),
            Self::InvalidStartTime => write!(formatter, "start wall clock must be nonnegative"),
            Self::Closed(reason) => write!(formatter, "ledger permanently closed: {reason}"),
            Self::InvalidCallerId => write!(formatter, "caller id is not an opaque bounded id"),
            Self::DuplicateCaller => write!(formatter, "caller id already has a reservation"),
            Self::InvalidInputBytes => write!(formatter, "input bytes out of range"),
            Self::InvalidFormatAttempt => write!(formatter, "format attempt must be 1 or 2"),
            Self::CallLimitExceeded => write!(formatter, "call budget exceeded"),
            Self::InputLimitExceeded => write!(formatter, "input-byte budget exceeded"),
            Self::UnknownSequence => write!(formatter, "unknown reservation sequence"),
            Self::AlreadyConsumed => write!(formatter, "reservation already consumed"),
            Self::ReservationMismatch => write!(formatter, "reservation metadata mismatch"),
        }
    }
}
impl std::error::Error for BudgetError {}

/// Read-only snapshot of cumulative budget consumption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetUsage {
    pub used_calls: u32,
    pub max_calls: u32,
    pub used_input_bytes: u64,
    pub max_input_bytes: u64,
}

/// Receipt for one model-attempt reservation. Returned live by
/// [`ModelCallLedger::reserve`] and consumed by
/// [`ModelCallLedger::consume`]; it is a local correlation receipt, not a
/// security credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelReservation {
    pub sequence: u64,
    pub caller_id: String,
    pub input_bytes: u64,
    pub format_attempt: u8,
    pub usage: BudgetUsage,
}

struct ReservationRecord {
    sequence: u64,
    caller_id: String,
    input_bytes: u64,
    format_attempt: u8,
    consumed: bool,
}

/// Cumulative budget ledger for one bounded run of host-side model attempts.
pub struct ModelCallLedger {
    max_calls: u32,
    max_input_bytes: u64,
    deadline_ms: i64,
    last_observed_ms: i64,
    next_sequence: u64,
    used_calls: u32,
    used_input_bytes: u64,
    records: Vec<ReservationRecord>,
    closed_reason: Option<String>,
}

impl ModelCallLedger {
    /// Check completion authority without reserving or charging another call.
    pub fn verify(&mut self, clock: &dyn Fn() -> Result<i64, String>) -> Result<i64, BudgetError> {
        self.fail_if_closed()?;
        self.checked_now(clock)
    }

    /// Creates a ledger with host-chosen cumulative limits and a nonnegative
    /// start wall clock. The deadline is `started_at_ms + max_duration_ms`,
    /// computed with checked arithmetic, and is exclusive: valid observations
    /// lie in `started_at_ms..deadline_ms`.
    pub fn new(
        max_calls: u32,
        max_input_bytes: u64,
        max_duration_ms: u64,
        started_at_ms: i64,
    ) -> Result<Self, BudgetError> {
        if max_calls == 0 || max_calls > MAX_LEDGER_CALLS {
            return Err(BudgetError::InvalidLimits);
        }
        if max_input_bytes == 0 || max_input_bytes > MAX_LEDGER_INPUT_BYTES {
            return Err(BudgetError::InvalidLimits);
        }
        if max_duration_ms == 0 || max_duration_ms > MAX_LEDGER_DURATION_MS as u64 {
            return Err(BudgetError::InvalidLimits);
        }
        if started_at_ms < 0 {
            return Err(BudgetError::InvalidStartTime);
        }
        let deadline_ms = started_at_ms
            .checked_add(max_duration_ms as i64)
            .ok_or(BudgetError::InvalidLimits)?;
        Ok(Self {
            max_calls,
            max_input_bytes,
            deadline_ms,
            last_observed_ms: started_at_ms,
            next_sequence: 0,
            used_calls: 0,
            used_input_bytes: 0,
            records: Vec::new(),
            closed_reason: None,
        })
    }

    /// Admits one model attempt and charges it immediately.
    ///
    /// Admission order: closed check, caller-id shape, input bytes, format
    /// attempt, duplicate caller id, then the cumulative call and input-byte
    /// limits. Every rejection so far is preflight: no clock read, no charge,
    /// no state change. The checked clock is then consulted twice: an initial
    /// reading validates the admission, then one call and all of `input_bytes`
    /// are consumed at once and the bounded record is stored, and a final
    /// reading runs before the receipt is returned. A failed or invalid
    /// reading at either point closes the ledger permanently; a charge made
    /// before a failed final reading is kept — the charge is kept even if the
    /// attempt is later unused, cancelled or fails.
    pub fn reserve(
        &mut self,
        caller_id: &str,
        input_bytes: u64,
        format_attempt: u8,
        clock: &dyn Fn() -> Result<i64, String>,
    ) -> Result<ModelReservation, BudgetError> {
        self.fail_if_closed()?;
        if !valid_caller_id(caller_id) {
            return Err(BudgetError::InvalidCallerId);
        }
        if input_bytes == 0 || input_bytes > MAX_RESERVATION_INPUT_BYTES {
            return Err(BudgetError::InvalidInputBytes);
        }
        if format_attempt != 1 && format_attempt != 2 {
            return Err(BudgetError::InvalidFormatAttempt);
        }
        if self
            .records
            .iter()
            .any(|record| record.caller_id == caller_id)
        {
            return Err(BudgetError::DuplicateCaller);
        }
        let charged_calls = self
            .used_calls
            .checked_add(1)
            .ok_or(BudgetError::CallLimitExceeded)?;
        if charged_calls > self.max_calls {
            return Err(BudgetError::CallLimitExceeded);
        }
        let charged_bytes = self
            .used_input_bytes
            .checked_add(input_bytes)
            .ok_or(BudgetError::InputLimitExceeded)?;
        if charged_bytes > self.max_input_bytes {
            return Err(BudgetError::InputLimitExceeded);
        }
        self.checked_now(clock)?;
        // Success: charge immediately and keep the charge unconditionally.
        self.used_calls = charged_calls;
        self.used_input_bytes = charged_bytes;
        let sequence = self.next_sequence;
        // Records are bounded by max_calls (<= MAX_LEDGER_CALLS), so the
        // sequence counter can never approach u64 overflow.
        self.next_sequence += 1;
        self.records.push(ReservationRecord {
            sequence,
            caller_id: caller_id.to_string(),
            input_bytes,
            format_attempt,
            consumed: false,
        });
        // Final check before the receipt is handed out: a revocation, expiry
        // or backwards move observed while the admitted attempt was being set
        // up can never return a lease. The charge above is kept and the ledger
        // closes permanently.
        self.checked_now(clock)?;
        Ok(ModelReservation {
            sequence,
            caller_id: caller_id.to_string(),
            input_bytes,
            format_attempt,
            usage: self.usage(),
        })
    }

    /// Redeems one reservation exactly once.
    ///
    /// The sequence must exist and still be live, and the expected input
    /// bytes/format attempt must match the stored metadata; those caller
    /// errors never close the ledger. The checked clock is consulted twice:
    /// an initial reading validates the redemption, then the record is marked
    /// consumed, and a final reading runs before any metadata is returned. A
    /// revocation, expiry or backwards move observed during the attempt closes
    /// the ledger permanently without ever returning a lease; the earlier
    /// charge is kept either way.
    pub fn consume(
        &mut self,
        sequence: u64,
        expected_input_bytes: u64,
        expected_format_attempt: u8,
        clock: &dyn Fn() -> Result<i64, String>,
    ) -> Result<ModelReservation, BudgetError> {
        self.fail_if_closed()?;
        let record = self
            .records
            .iter()
            .find(|record| record.sequence == sequence)
            .ok_or(BudgetError::UnknownSequence)?;
        if record.consumed {
            return Err(BudgetError::AlreadyConsumed);
        }
        if record.input_bytes != expected_input_bytes
            || record.format_attempt != expected_format_attempt
        {
            return Err(BudgetError::ReservationMismatch);
        }
        let caller_id = record.caller_id.clone();
        let input_bytes = record.input_bytes;
        let format_attempt = record.format_attempt;
        self.checked_now(clock)?;
        let record = self
            .records
            .iter_mut()
            .find(|record| record.sequence == sequence)
            .expect("record present: it was found above and checked_now only appends a close");
        record.consumed = true;
        // Final check before the metadata is handed out: a revocation, expiry
        // or backwards move observed while the attempt was being redeemed can
        // never return a lease. The record stays consumed and the earlier
        // charge is kept; the ledger closes permanently.
        self.checked_now(clock)?;
        Ok(ModelReservation {
            sequence,
            caller_id,
            input_bytes,
            format_attempt,
            usage: self.usage(),
        })
    }

    /// Read-only snapshot of cumulative consumption.
    pub fn usage(&self) -> BudgetUsage {
        BudgetUsage {
            used_calls: self.used_calls,
            max_calls: self.max_calls,
            used_input_bytes: self.used_input_bytes,
            max_input_bytes: self.max_input_bytes,
        }
    }

    /// Permanently closes the ledger with a host reason. Idempotent: only the
    /// first close takes effect and its reason is kept verbatim; all later
    /// `reserve`/`consume` calls fail with [`BudgetError::Closed`]. Charges
    /// taken before the close remain visible in [`ModelCallLedger::usage`].
    pub fn close(&mut self, reason: &str) -> bool {
        if self.closed_reason.is_some() {
            return false;
        }
        self.closed_reason = Some(reason.to_string());
        true
    }

    /// The exact reason recorded by the first close, if any.
    pub fn closed_reason(&self) -> Option<&str> {
        self.closed_reason.as_deref()
    }

    fn fail_if_closed(&self) -> Result<(), BudgetError> {
        match &self.closed_reason {
            Some(reason) => Err(BudgetError::Closed(reason.clone())),
            None => Ok(()),
        }
    }

    /// Validates one clock reading against monotonicity and the exclusive
    /// deadline, updating the last observed time only on success. Any failure
    /// terminates the ledger permanently with the reason preserved exactly.
    fn checked_now(&mut self, clock: &dyn Fn() -> Result<i64, String>) -> Result<i64, BudgetError> {
        let now_ms = match clock() {
            Ok(now_ms) => now_ms,
            Err(reason) => return Err(self.terminate(reason)),
        };
        if now_ms < 0 {
            return Err(self.terminate(format!(
                "model call ledger closed: clock returned negative wall time {now_ms}"
            )));
        }
        if now_ms < self.last_observed_ms {
            return Err(self.terminate(format!(
                "model call ledger closed: wall time moved backwards from {} to {now_ms}",
                self.last_observed_ms
            )));
        }
        if now_ms >= self.deadline_ms {
            return Err(self.terminate(format!(
                "model call ledger closed: wall time {now_ms} reached the exclusive deadline {}",
                self.deadline_ms
            )));
        }
        self.last_observed_ms = now_ms;
        Ok(now_ms)
    }

    /// Permanent close with the reason preserved verbatim. A caller-supplied
    /// reason (clock/grant failure) passes through byte-for-byte; internal
    /// reasons are plain descriptive text, never sentinel markers.
    fn terminate(&mut self, reason: String) -> BudgetError {
        if self.closed_reason.is_none() {
            self.closed_reason = Some(reason.clone());
        }
        BudgetError::Closed(reason)
    }
}

/// Caller ids are opaque: never trimmed or canonicalized. Surrounding
/// whitespace and control characters are rejected outright, and the ORIGINAL
/// byte length is bounded so padding cannot smuggle an over-limit id past the
/// check. Identity and hash validation stay with the host registry.
fn valid_caller_id(caller_id: &str) -> bool {
    !caller_id.is_empty()
        && caller_id.len() <= MAX_CALLER_ID_BYTES
        && caller_id.trim() == caller_id
        && !caller_id.chars().any(char::is_control)
}

#[cfg(test)]
#[path = "memory-model-budget-tests.rs"]
mod tests;
