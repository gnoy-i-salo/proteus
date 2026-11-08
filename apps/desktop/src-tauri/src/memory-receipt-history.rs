//! Retained native audit results for later model-context assembly. This store
//! grants no access: its session owner rechecks authority before reuse.
use crate::memory::session::RecordedRetrieval;
use std::{
    collections::{HashMap, HashSet},
    io,
    sync::Arc,
};
const MAX_RECEIPTS: usize = 1000;
const MAX_RETAINED_BYTES: usize = 16 * 1024 * 1024;
const INVALID: &str = "Квитанция памяти недоступна или превышает лимит.";
#[derive(Default)]
pub(super) struct ReceiptHistory {
    records: HashMap<String, Arc<RecordedRetrieval>>,
    bytes: usize,
}
impl ReceiptHistory {
    pub(super) fn insert(
        &mut self,
        record: RecordedRetrieval,
    ) -> Result<Arc<RecordedRetrieval>, String> {
        let id = &record.receipt.id;
        if id.is_empty()
            || id.len() > 128
            || self.records.contains_key(id)
            || self.records.len() >= MAX_RECEIPTS
        {
            return Err(INVALID.into());
        }
        // Count actual serialized bytes without allocating another copy of the
        // text. Escaping and all metadata count, not just receipt.sourceBytes.
        let mut counter = ByteCounter {
            used: 0,
            remaining: MAX_RETAINED_BYTES - self.bytes,
        };
        serde_json::to_writer(&mut counter, &record).map_err(|_| INVALID)?;
        self.bytes += counter.used;
        let record = Arc::new(record);
        self.records
            .insert(record.receipt.id.clone(), record.clone());
        Ok(record)
    }
    pub(super) fn select(&self, ids: &[String]) -> Result<Vec<Arc<RecordedRetrieval>>, String> {
        if ids.len() > MAX_RECEIPTS {
            return Err(INVALID.into());
        }
        let mut seen = HashSet::new();
        let mut result = Vec::with_capacity(ids.len());
        for id in ids {
            if id.is_empty() || id.len() > 128 || !seen.insert(id) {
                return Err(INVALID.into());
            }
            result.push(self.records.get(id).ok_or(INVALID)?.clone());
        }
        Ok(result)
    }
}
struct ByteCounter {
    used: usize,
    remaining: usize,
}
impl io::Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::other(INVALID));
        }
        self.remaining -= bytes.len();
        self.used += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[cfg(test)]
#[path = "memory-receipt-history-tests.rs"]
mod tests;
