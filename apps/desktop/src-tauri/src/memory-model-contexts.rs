//! Single-use model input bindings. The session owner supplies authorized source
//! receipts and a checked clock; this owner never grants corpus access itself.
use crate::memory::{
    model_budget::{BudgetUsage, ModelCallLedger},
    session::{PreparedModelInput, RecordedRetrieval},
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompletedModelContext {
    pub schema_version: u8,
    pub lease_id: String,
    pub invocation_id: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
    pub operation: String,
    pub input_sha256: String,
    pub input_bytes: usize,
    pub context_sha256: String,
    pub receipt_ids: Vec<String>,
    pub format_attempt: u8,
    pub completed_at_ms: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvocationBinding {
    pub provider: String,
    pub model: String,
    pub effort: String,
    pub operation: String,
}
pub struct DispatchInput<'a> {
    pub invocation_id: &'a str,
    pub binding: &'a InvocationBinding,
    pub system_prompt: &'a str,
    pub task: &'a str,
}
pub struct ReservedContext {
    pub id: String,
    pub input: Arc<PreparedModelInput>,
    pub usage: BudgetUsage,
}
struct ContextEntry {
    sequence: u64,
    format_attempt: u8,
    input: Arc<PreparedModelInput>,
    receipts: Vec<Arc<RecordedRetrieval>>,
    invocation_id: Option<String>,
    completed_at_ms: Option<i64>,
}
pub struct ModelContexts {
    binding: InvocationBinding,
    ledger: ModelCallLedger,
    entries: HashMap<String, ContextEntry>,
    invocations: HashSet<String>,
}
const INVALID: &str = "Контекст модельного вызова недоступен или не соответствует запросу.";
impl ModelContexts {
    pub fn completed_audit(
        &mut self,
        ids: &[String],
        clock: &dyn Fn() -> Result<i64, String>,
    ) -> Result<Vec<CompletedModelContext>, String> {
        let entries = Self::completed_entries(&self.entries, ids)?;
        self.ledger.verify(clock).map_err(|e| e.to_string())?;
        let mut audit = Vec::with_capacity(entries.len());
        for (id, entry) in entries {
            self.ledger.verify(clock).map_err(|e| e.to_string())?;
            audit.push(CompletedModelContext {
                schema_version: 1,
                lease_id: id.clone(),
                invocation_id: entry.invocation_id.clone().ok_or(INVALID)?,
                provider: self.binding.provider.clone(),
                model: self.binding.model.clone(),
                effort: self.binding.effort.clone(),
                operation: self.binding.operation.clone(),
                input_sha256: entry.input.identity.input_sha256.clone(),
                input_bytes: entry.input.identity.input_bytes,
                context_sha256: entry.input.context_sha256.clone(),
                receipt_ids: entry
                    .receipts
                    .iter()
                    .map(|r| r.receipt.id.clone())
                    .collect(),
                format_attempt: entry.format_attempt,
                completed_at_ms: entry.completed_at_ms.ok_or(INVALID)?,
            });
        }
        self.ledger.verify(clock).map_err(|e| e.to_string())?;
        Ok(audit)
    }
    pub fn verify(&mut self, clock: &dyn Fn() -> Result<i64, String>) -> Result<(), String> {
        self.ledger
            .verify(clock)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    pub fn new(
        binding: InvocationBinding,
        calls: u32,
        bytes: u64,
        duration_ms: u64,
        started: i64,
    ) -> Result<Self, String> {
        if !["codex", "kimi"].contains(&binding.provider.as_str())
            || !["low", "medium", "high"].contains(&binding.effort.as_str())
            || !["corpus-analysis", "persona-reply"].contains(&binding.operation.as_str())
            || binding.model.trim().is_empty()
            || binding.model.len() > 120
            || binding.model.contains(['\n', '\r', '\0'])
        {
            return Err(INVALID.into());
        }
        let ledger =
            ModelCallLedger::new(calls, bytes, duration_ms, started).map_err(|e| e.to_string())?;
        Ok(Self {
            binding,
            ledger,
            entries: HashMap::new(),
            invocations: HashSet::new(),
        })
    }
    pub fn reserve(
        &mut self,
        system_prompt: &str,
        task: &str,
        receipts: Vec<Arc<RecordedRetrieval>>,
        format_attempt: u8,
        clock: &dyn Fn() -> Result<i64, String>,
    ) -> Result<ReservedContext, String> {
        let mut seen = HashSet::new();
        if receipts.len() > 1000 || receipts.iter().any(|r| !seen.insert(&r.receipt.id)) {
            return Err(INVALID.into());
        }
        let input = Arc::new(crate::memory::context_input::prepare(
            system_prompt,
            task,
            &receipts,
            &[],
        )?);
        let id = format!(
            "modelContext:{}",
            crate::connections::storage::random_id(24)?
        );
        if self.entries.contains_key(&id) {
            return Err(INVALID.into());
        }
        let reservation = self
            .ledger
            .reserve(
                &id,
                input.identity.input_bytes as u64,
                format_attempt,
                clock,
            )
            .map_err(|e| e.to_string())?;
        self.entries.insert(
            id.clone(),
            ContextEntry {
                sequence: reservation.sequence,
                format_attempt,
                input: input.clone(),
                receipts,
                invocation_id: None,
                completed_at_ms: None,
            },
        );
        self.ledger.verify(clock).map_err(|e| e.to_string())?;
        Ok(ReservedContext {
            id,
            input,
            usage: reservation.usage,
        })
    }
    pub fn consume(
        &mut self,
        id: &str,
        dispatch: DispatchInput<'_>,
        clock: &dyn Fn() -> Result<i64, String>,
    ) -> Result<(), String> {
        if id.len() > 128
            || dispatch.binding != &self.binding
            || dispatch.invocation_id.is_empty()
            || dispatch.invocation_id.len() > 80
            || !dispatch
                .invocation_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || self.invocations.contains(dispatch.invocation_id)
        {
            return Err(INVALID.into());
        }
        let entry = self.entries.get(id).ok_or(INVALID)?;
        if entry.invocation_id.is_some()
            || crate::memory::model_input::model_input_identity(
                dispatch.system_prompt,
                dispatch.task,
            )? != entry.input.identity
        {
            return Err(INVALID.into());
        }
        self.ledger
            .consume(
                entry.sequence,
                entry.input.identity.input_bytes as u64,
                entry.format_attempt,
                clock,
            )
            .map_err(|e| e.to_string())?;
        self.entries.get_mut(id).ok_or(INVALID)?.invocation_id =
            Some(dispatch.invocation_id.into());
        self.invocations.insert(dispatch.invocation_id.into());
        self.ledger.verify(clock).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn check_running(
        &mut self,
        id: &str,
        invocation_id: &str,
        clock: &dyn Fn() -> Result<i64, String>,
    ) -> Result<(), String> {
        if id.len() > 128 || invocation_id.len() > 80 {
            return Err(INVALID.into());
        }
        let entry = self.entries.get(id).ok_or(INVALID)?;
        if entry.completed_at_ms.is_some() || entry.invocation_id.as_deref() != Some(invocation_id)
        {
            return Err(INVALID.into());
        }
        self.ledger
            .verify(clock)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    /// Only native dispatch calls this after its CLI execution succeeds.
    pub fn complete(
        &mut self,
        id: &str,
        invocation_id: &str,
        clock: &dyn Fn() -> Result<i64, String>,
    ) -> Result<(), String> {
        self.check_running(id, invocation_id, clock)?;
        let completed_at_ms = self.ledger.verify(clock).map_err(|e| e.to_string())?;
        self.entries.get_mut(id).ok_or(INVALID)?.completed_at_ms = Some(completed_at_ms);
        Ok(())
    }
    pub fn completed_receipts(
        &mut self,
        ids: &[String],
        clock: &dyn Fn() -> Result<i64, String>,
    ) -> Result<Vec<Arc<RecordedRetrieval>>, String> {
        let entries = Self::completed_entries(&self.entries, ids)?;
        self.ledger.verify(clock).map_err(|e| e.to_string())?;
        let mut seen_receipts = HashSet::new();
        let mut receipts = Vec::new();
        for (_, entry) in entries {
            self.ledger.verify(clock).map_err(|e| e.to_string())?;
            for receipt in &entry.receipts {
                if seen_receipts.insert(&receipt.receipt.id) {
                    receipts.push(receipt.clone());
                }
            }
        }
        self.ledger.verify(clock).map_err(|e| e.to_string())?;
        Ok(receipts)
    }
    fn completed_entries<'a>(
        entries_by_id: &'a HashMap<String, ContextEntry>,
        ids: &'a [String],
    ) -> Result<Vec<(&'a String, &'a ContextEntry)>, String> {
        if ids.len() > 1000 {
            return Err(INVALID.into());
        }
        let mut seen_ids = HashSet::new();
        let mut entries = Vec::with_capacity(ids.len());
        for id in ids {
            if id.len() > 128 || !seen_ids.insert(id) {
                return Err(INVALID.into());
            }
            let entry = entries_by_id.get(id).ok_or(INVALID)?;
            if entry.completed_at_ms.is_none() {
                return Err(INVALID.into());
            }
            entries.push((id, entry));
        }
        Ok(entries)
    }
}
#[cfg(test)]
#[path = "memory-model-contexts-tests.rs"]
mod tests;
