//! Cumulative native retrieval accounting. Host-owned limits and measured
//! charges never come from model actions. One mutable ledger serializes calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecallLimits {
    pub max_calls: usize,
    pub max_candidates: usize,
    pub max_episodes: usize,
    pub max_source_bytes: usize,
    pub max_context_tokens: usize,
    pub max_duration_ms: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetrievalCharge {
    pub candidates: usize,
    pub episodes: usize,
    pub source_bytes: usize,
    pub context_tokens: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecallAllowance {
    pub remaining: RetrievalCharge,
    pub duration_ms: i64,
}
pub struct RecallLedger {
    limits: RecallLimits,
    used: RetrievalCharge,
    calls: usize,
    last_ms: i64,
    deadline: i64,
    blocked: bool,
}
impl RecallLedger {
    pub fn new(limits: RecallLimits, now_ms: i64) -> Result<Self, String> {
        if now_ms < 0
            || !(1..=1000).contains(&limits.max_calls)
            || !(1..=100000).contains(&limits.max_candidates)
            || !(1..=1000).contains(&limits.max_episodes)
            || !(1..=64 * 1024 * 1024).contains(&limits.max_source_bytes)
            || !(1..=1000000).contains(&limits.max_context_tokens)
            || !(1..=3_600_000).contains(&limits.max_duration_ms)
        {
            return Err("Неверный бюджет сессии памяти.".into());
        }
        let deadline = now_ms
            .checked_add(limits.max_duration_ms)
            .ok_or("Неверное время сессии памяти.")?;
        Ok(Self {
            limits,
            used: RetrievalCharge::default(),
            calls: 0,
            last_ms: now_ms,
            deadline,
            blocked: false,
        })
    }
    pub fn usage(&self) -> (usize, RetrievalCharge) {
        (self.calls, self.used)
    }
    /// Recheck authority after admitted work without charging another retrieval.
    /// This also closes the session on late expiry/revocation before disclosure.
    pub fn verify(&mut self, now_ms: impl Fn() -> Result<i64, String>) -> Result<i64, String> {
        if self.blocked {
            return Err("Сессия памяти завершена или её время истекло.".into());
        }
        let remaining = self.observe_checked(now_ms())?;
        Ok(self.deadline - remaining)
    }
    fn observe(&mut self, now: i64) -> Result<i64, String> {
        if self.blocked || now < self.last_ms || now >= self.deadline {
            self.blocked = true;
            return Err("Сессия памяти завершена или её время истекло.".into());
        }
        self.last_ms = now;
        Ok(self.deadline - now)
    }
    fn observe_checked(&mut self, now: Result<i64, String>) -> Result<i64, String> {
        match now {
            Ok(now) => self.observe(now),
            Err(reason) => {
                self.blocked = true;
                Err(reason)
            }
        }
    }
    pub fn run<T>(
        &mut self,
        now_ms: impl Fn() -> i64,
        action: impl FnOnce(RecallAllowance) -> Result<(T, RetrievalCharge), String>,
    ) -> Result<T, String> {
        self.run_checked(|| Ok(now_ms()), action)
    }
    /// Admitted attempts consume a call; denied preflight never starts work.
    /// Only successful disclosures consume text
    /// accounting; malformed host charges or invalid time permanently close it.
    /// The caller still checks live authorization/cancellation before disclosure.
    pub fn run_checked<T>(
        &mut self,
        now_ms: impl Fn() -> Result<i64, String>,
        action: impl FnOnce(RecallAllowance) -> Result<(T, RetrievalCharge), String>,
    ) -> Result<T, String> {
        if self.blocked {
            return Err("Сессия памяти завершена или её время истекло.".into());
        }
        let remaining = RetrievalCharge {
            candidates: self.limits.max_candidates - self.used.candidates,
            episodes: self.limits.max_episodes - self.used.episodes,
            source_bytes: self.limits.max_source_bytes - self.used.source_bytes,
            context_tokens: self.limits.max_context_tokens - self.used.context_tokens,
        };
        if self.calls >= self.limits.max_calls
            || [
                remaining.candidates,
                remaining.episodes,
                remaining.source_bytes,
                remaining.context_tokens,
            ]
            .contains(&0)
        {
            self.blocked = true;
            return Err("Исчерпан совокупный бюджет памяти.".into());
        }
        let duration_ms = self.observe_checked(now_ms())?;
        self.calls += 1;
        let result = action(RecallAllowance {
            remaining,
            duration_ms,
        });
        let finished = self.observe_checked(now_ms());
        let (value, charge) = result?;
        finished?;
        if charge.candidates > remaining.candidates
            || charge.episodes > remaining.episodes
            || charge.source_bytes > remaining.source_bytes
            || charge.context_tokens > remaining.context_tokens
        {
            self.blocked = true;
            return Err("Результат превысил выделенный бюджет памяти.".into());
        }
        self.used.candidates += charge.candidates;
        self.used.episodes += charge.episodes;
        self.used.source_bytes += charge.source_bytes;
        self.used.context_tokens += charge.context_tokens;
        Ok(value)
    }
}
#[cfg(test)]
#[path = "memory-budget-tests.rs"]
mod tests;
