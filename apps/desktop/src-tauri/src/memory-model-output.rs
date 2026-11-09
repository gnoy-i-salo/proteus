//! Exact native output identity, separate from persisted input/receipt audit.
use super::{ModelContexts, INVALID};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct OutputIdentity {
    bytes: usize,
    sha256: [u8; 32],
}
impl OutputIdentity {
    fn new(answer: &str) -> Result<Self, String> {
        // The CLI reader already limits its complete stream to 4 MiB. Bound
        // direct native callers too, without retaining a second answer copy.
        if answer.len() > 4 * 1024 * 1024 || answer.trim().is_empty() {
            return Err("Неверный размер завершённого ответа модели.".into());
        }
        Ok(Self {
            bytes: answer.len(),
            sha256: Sha256::digest(answer.as_bytes()).into(),
        })
    }
}
impl ModelContexts {
    /// Only native execution supplies this answer after the bound CLI succeeds.
    /// No caller hash or completion flag can stand in for its exact UTF-8 bytes.
    pub fn complete_with_output(
        &mut self,
        id: &str,
        invocation_id: &str,
        answer: &str,
        clock: &dyn Fn() -> Result<i64, String>,
    ) -> Result<(), String> {
        let output = OutputIdentity::new(answer)?;
        self.complete(id, invocation_id, clock)?;
        self.entries.get_mut(id).ok_or(INVALID)?.output = Some(output);
        Ok(())
    }
    pub fn check_completed_output(
        &mut self,
        id: &str,
        invocation_id: &str,
        answer: &str,
        clock: &dyn Fn() -> Result<i64, String>,
    ) -> Result<(), String> {
        if id.len() > 128 || invocation_id.len() > 80 {
            return Err(INVALID.into());
        }
        let entry = self.entries.get(id).ok_or(INVALID)?;
        if entry.completed_at_ms.is_none() || entry.invocation_id.as_deref() != Some(invocation_id)
        {
            return Err(INVALID.into());
        }
        let expected = entry.output.ok_or(INVALID)?;
        self.ledger.verify(clock).map_err(|e| e.to_string())?;
        if OutputIdentity::new(answer)? != expected {
            return Err("Текст не соответствует завершённому ответу CLI.".into());
        }
        self.ledger.verify(clock).map_err(|e| e.to_string())?;
        Ok(())
    }
}
#[cfg(test)]
#[path = "memory-model-output-tests.rs"]
mod tests;
