//! Stream identity must be complete and budgeted before a preview or publication
//! can trust it. An open descriptor alone does not prevent in-place file edits.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputFingerprint {
    pub sha256: String,
    pub byte_length: u64,
}
pub struct FingerprintReader<'a, R> {
    reader: R,
    hash: Sha256,
    bytes: u64,
    max_bytes: u64,
    duration: Duration,
    started: Instant,
    cancelled: &'a AtomicBool,
    eof: bool,
    failed: bool,
}
impl<'a, R: Read> FingerprintReader<'a, R> {
    pub fn new(
        reader: R,
        max_bytes: u64,
        duration: Duration,
        cancelled: &'a AtomicBool,
    ) -> io::Result<Self> {
        if max_bytes == 0 || max_bytes > 9_007_199_254_740_991 || duration.is_zero() {
            return Err(io::Error::other("Неверный бюджет чтения источника."));
        }
        Ok(Self {
            reader,
            hash: Sha256::new(),
            bytes: 0,
            max_bytes,
            duration,
            started: Instant::now(),
            cancelled,
            eof: false,
            failed: false,
        })
    }
    fn check(&self) -> io::Result<()> {
        if self.failed {
            return Err(io::Error::other(
                "Предыдущее чтение источника завершилось ошибкой.",
            ));
        }
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(io::Error::other("Чтение источника отменено."));
        }
        if self.started.elapsed() >= self.duration {
            return Err(io::Error::other("Превышено время чтения источника."));
        }
        Ok(())
    }
    pub fn finish(self) -> io::Result<InputFingerprint> {
        self.check()?;
        if !self.eof {
            return Err(io::Error::other("Источник прочитан не полностью."));
        }
        Ok(InputFingerprint {
            sha256: format!("{:x}", self.hash.finalize()),
            byte_length: self.bytes,
        })
    }
    fn read_checked(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        if buffer.is_empty() || self.eof {
            return Ok(0);
        }
        // Probe one byte past the input limit to distinguish exact EOF from a
        // truncated prefix. Excess bytes are never returned to the consumer.
        let remaining = self.max_bytes - self.bytes + 1;
        let limit = (buffer.len().min(65536) as u64).min(remaining) as usize;
        let count = self.reader.read(&mut buffer[..limit])?;
        self.check()?;
        if count > limit || count as u64 > self.max_bytes - self.bytes {
            return Err(io::Error::other("Превышен размер выбранного источника."));
        }
        if count == 0 {
            self.eof = true;
        }
        self.hash.update(&buffer[..count]);
        self.bytes += count as u64;
        Ok(count)
    }
}
impl<R: Read> Read for FingerprintReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let result = self.read_checked(buffer);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}
#[cfg(test)]
#[path = "ingestion-fingerprint-tests.rs"]
mod tests;
