//! Actual input I/O failures are distinct from parser, scope, cancellation and budget failures.
use std::{
    cell::Cell,
    io::{self, Read},
};
#[derive(Debug)]
pub struct Failure {
    pub message: String,
    pub retryable: bool,
}
pub(crate) struct TrackedReader<'a, R> {
    pub reader: R,
    pub failed: &'a Cell<bool>,
}
impl<R: Read> Read for TrackedReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.reader
            .read(buffer)
            .inspect_err(|_| self.failed.set(true))
    }
}
