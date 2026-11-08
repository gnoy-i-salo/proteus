//! Verified, content-addressed installation of pinned model artifacts into a
//! native-owned local cache. This is a building block for the model manager:
//! it installs bytes supplied by the caller's reader; it never downloads,
//! never runs inference, and never reads CLI or credential state.
//!
//! The installed artifact path is derived from the pin (`<cache-dir>/<sha256>`),
//! so a caller-provided arbitrary artifact filename is not part of this API.
//! A returned path always names a fully verified artifact; a partial or
//! unverified file is never exposed at that path.
//!
//! Guarantees:
//! - Bounded streaming: at most `INSTALL_CHUNK_BYTES` per read, with checked
//!   byte counters; the whole artifact is never held in memory here.
//! - Truncated, oversized, and checksum-mismatched streams are rejected, and
//!   their temporary files are removed; oversized streams are rejected as soon
//!   as the pinned byte length is exceeded.
//! - The cooperative `check` runs before any cache IO, before and after every
//!   read/write cycle, once after EOF, and once immediately before publication;
//!   re-verification of an already published artifact applies the same
//!   before/after-read cadence and the same post-EOF recheck. A failing check
//!   aborts with `CacheInstallError::Cancelled` and publishes nothing.
//! - Publication is atomic where the OS supports hard links: the verified,
//!   fsynced temporary file is hard-linked into place (same filesystem, because
//!   the temporary directory lives inside the cache directory), so concurrent
//!   installers of the same pin complete by re-verifying the winner's file
//!   instead of overwriting it. A failed or cancelled install never destroys
//!   an existing verified artifact, and replay of an already verified pin
//!   reuses the cache without touching the reader.
//! - A corrupt file already occupying the content-addressed path is reported
//!   as `CacheInstallError::ExistingArtifactCorrupt` and left untouched for
//!   the owning manager to inspect or remove; this module never destroys data
//!   it cannot verify.
//! - Directory and symlink misuse: the cache directory and the artifact path
//!   must be a real directory and a regular file. A symlinked cache directory
//!   is refused, and anything that is not a regular file at the artifact path
//!   (directory, symlink, fifo, socket, device) is refused before any source
//!   byte is consumed.
//!
//! Limitations (deliberate, documented):
//! - This does not protect against a malicious local process with identical OS
//!   privileges; such a process can modify or replace cache contents outside
//!   this module. Intermediate path components are not validated.
//! - fsync covers the temporary file and, on Unix, a best-effort directory
//!   fsync after publication; crash-durability of directory metadata is
//!   best-effort and OS-dependent.
//! - If the process dies mid-install, an orphaned temporary directory may
//!   remain in the cache directory. It contains only content destined for this
//!   cache (partial or verified bytes under a random name) and is safe for the
//!   owner to remove; cleanup at restart is the manager's responsibility.
//! - Installers that do not follow this publication protocol can still race a
//!   partially written file into the artifact path; the re-verification after
//!   `AlreadyExists` then reports `ExistingArtifactCorrupt` and never exposes
//!   the partial file.
//! - Timeouts/deadlines are expressed by the caller through `check`; this
//!   module runs no timer of its own.
//! - `lookup_verified` is the read-only inspection entry point: it never
//!   creates the cache directory, never modifies existing bytes, and never
//!   touches a source reader. A missing cache root or artifact reports `None`;
//!   a malformed artifact occupying the path is an explicit error.
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

/// Bounded read chunk for streaming; the artifact is never buffered whole.
pub const INSTALL_CHUNK_BYTES: usize = 1024 * 1024;
const TEMP_DIR_PREFIX: &str = ".artifact-install-";
const TEMP_DIR_ATTEMPTS: usize = 8;

/// Trusted byte length and lowercase hex SHA-256 of one model artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactPin {
    pub bytes: u64,
    pub sha256: String,
}
impl ArtifactPin {
    pub fn new(bytes: u64, sha256: impl Into<String>) -> Result<Self, CacheInstallError> {
        let pin = Self {
            bytes,
            sha256: sha256.into(),
        };
        pin.validate()?;
        Ok(pin)
    }
    fn validate(&self) -> Result<(), CacheInstallError> {
        let digest = self.sha256.len() == 64
            && self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if self.bytes == 0 || !digest {
            return Err(CacheInstallError::InvalidPin);
        }
        Ok(())
    }
}
/// Failure modes of a cache install. Typed variants carry no secrets.
#[derive(Debug)]
pub enum CacheInstallError {
    /// Pin is structurally invalid (zero bytes or non-lowercase-hex digest).
    InvalidPin,
    /// The cooperative cancellation/deadline check aborted the install.
    Cancelled(String),
    /// Cache directory is missing, not a directory, or a symlink.
    CacheDirectoryInvalid,
    /// A directory, symlink, or other non-regular file occupies the
    /// content-addressed artifact path.
    ArtifactPathBlocked,
    /// The artifact path already holds data that does not match the pin;
    /// the file is preserved untouched for the owning manager.
    ExistingArtifactCorrupt,
    /// The supplied reader failed mid-stream.
    SourceFailed(io::Error),
    /// The stream ended before the pinned byte length.
    ArtifactTruncated,
    /// The stream exceeded the pinned byte length; rejected on the first
    /// bounded read that crosses it.
    ArtifactOversized,
    /// Byte length matched but the SHA-256 digest did not.
    ArtifactChecksumMismatch,
    /// Other local filesystem failure.
    Io(io::Error),
}
impl std::fmt::Display for CacheInstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPin => write!(formatter, "invalid artifact pin"),
            Self::Cancelled(reason) => write!(formatter, "install cancelled: {reason}"),
            Self::CacheDirectoryInvalid => {
                write!(formatter, "cache directory is not a real directory")
            }
            Self::ArtifactPathBlocked => {
                write!(formatter, "non-regular file blocks the artifact path")
            }
            Self::ExistingArtifactCorrupt => {
                write!(formatter, "existing artifact does not match the pin")
            }
            Self::SourceFailed(error) => write!(formatter, "source read failed: {error}"),
            Self::ArtifactTruncated => write!(formatter, "artifact stream is truncated"),
            Self::ArtifactOversized => write!(formatter, "artifact stream exceeds pinned size"),
            Self::ArtifactChecksumMismatch => {
                write!(formatter, "artifact digest does not match the pin")
            }
            Self::Io(error) => write!(formatter, "cache filesystem error: {error}"),
        }
    }
}
impl std::error::Error for CacheInstallError {}

/// Streams `reader` into an exclusive temporary file inside `cache_dir`,
/// verifies the exact pinned length and SHA-256, fsyncs, and atomically
/// publishes the artifact at the content-addressed path
/// `<cache-dir>/<sha256>`. If a verified artifact already occupies that path,
/// the reader is not consumed and the existing path is returned.
///
/// `check` is cooperative cancellation/deadline input owned by the caller; it
/// runs before any cache IO, before and after every read/write cycle, once
/// after EOF, and once immediately before publication.
pub fn install_verified_artifact(
    cache_dir: &Path,
    pin: &ArtifactPin,
    reader: &mut dyn Read,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<PathBuf, CacheInstallError> {
    checked(check)?;
    pin.validate()?;
    prepare_cache_dir(cache_dir)?;
    let destination = artifact_path(cache_dir, pin);
    if let Some(metadata) = symlink_metadata_optional(&destination)? {
        if !metadata.file_type().is_file() {
            return Err(CacheInstallError::ArtifactPathBlocked);
        }
        verify_artifact(&destination, pin, check)?;
        return Ok(destination);
    }
    let temporary = TemporaryInstall::create(cache_dir)?;
    let result = stream_to_temporary(reader, pin, check, &temporary)
        .and_then(|()| publish(&temporary, &destination, pin, check));
    temporary.discard();
    result
}
fn checked(check: &dyn Fn() -> Result<(), String>) -> Result<(), CacheInstallError> {
    check().map_err(CacheInstallError::Cancelled)
}
/// Read-only inspection of the content-addressed artifact path: returns
/// `Ok(Some(path))` only when a fully re-verified artifact already occupies
/// `<cache-dir>/<sha256>`. Returns `Ok(None)` when the cache root or the
/// artifact is missing; never creates the cache root and never consumes a
/// source reader. Existing malformed artifacts and blocked paths report the
/// same explicit errors as [`install_verified_artifact`].
pub fn lookup_verified(
    cache_dir: &Path,
    pin: &ArtifactPin,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Option<PathBuf>, CacheInstallError> {
    checked(check)?;
    pin.validate()?;
    match fs::symlink_metadata(cache_dir) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(CacheInstallError::CacheDirectoryInvalid);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(CacheInstallError::Io(error)),
    }
    let destination = artifact_path(cache_dir, pin);
    match symlink_metadata_optional(&destination)? {
        None => Ok(None),
        Some(_) => {
            verify_artifact(&destination, pin, check)?;
            Ok(Some(destination))
        }
    }
}
fn artifact_path(cache_dir: &Path, pin: &ArtifactPin) -> PathBuf {
    cache_dir.join(&pin.sha256)
}
fn symlink_metadata_optional(path: &Path) -> Result<Option<fs::Metadata>, CacheInstallError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(CacheInstallError::Io(error)),
    }
}
fn prepare_cache_dir(cache_dir: &Path) -> Result<(), CacheInstallError> {
    match fs::symlink_metadata(cache_dir) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(CacheInstallError::CacheDirectoryInvalid);
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(cache_dir).map_err(CacheInstallError::Io)
        }
        Err(error) => Err(CacheInstallError::Io(error)),
    }
}
fn stream_to_temporary(
    reader: &mut dyn Read,
    pin: &ArtifactPin,
    check: &dyn Fn() -> Result<(), String>,
    temporary: &TemporaryInstall,
) -> Result<(), CacheInstallError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary.file_path())
        .map_err(CacheInstallError::Io)?;
    let mut hash = Sha256::new();
    let mut total: u64 = 0;
    let mut buffer = vec![0u8; INSTALL_CHUNK_BYTES];
    loop {
        checked(check)?;
        let read = reader
            .read(&mut buffer)
            .map_err(CacheInstallError::SourceFailed)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or(CacheInstallError::ArtifactOversized)?;
        if total > pin.bytes {
            return Err(CacheInstallError::ArtifactOversized);
        }
        hash.update(&buffer[..read]);
        file.write_all(&buffer[..read])
            .map_err(CacheInstallError::Io)?;
        checked(check)?;
    }
    checked(check)?;
    if total != pin.bytes {
        return Err(CacheInstallError::ArtifactTruncated);
    }
    if format!("{:x}", hash.finalize()) != pin.sha256 {
        return Err(CacheInstallError::ArtifactChecksumMismatch);
    }
    file.sync_all().map_err(CacheInstallError::Io)
}
fn publish(
    temporary: &TemporaryInstall,
    destination: &Path,
    pin: &ArtifactPin,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<PathBuf, CacheInstallError> {
    checked(check)?;
    match fs::hard_link(temporary.file_path(), destination) {
        Ok(()) => {
            sync_parent_dir_best_effort(destination);
            Ok(destination.to_path_buf())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            // A concurrent installer (or a prior run) published first; adopt
            // its file only after full re-verification against the pin.
            verify_artifact(destination, pin, check)?;
            Ok(destination.to_path_buf())
        }
        Err(error) => Err(CacheInstallError::Io(error)),
    }
}
fn verify_artifact(
    path: &Path,
    pin: &ArtifactPin,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(), CacheInstallError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => CacheInstallError::ExistingArtifactCorrupt,
        _ => CacheInstallError::Io(error),
    })?;
    if !metadata.file_type().is_file() {
        return Err(CacheInstallError::ArtifactPathBlocked);
    }
    if metadata.len() != pin.bytes {
        return Err(CacheInstallError::ExistingArtifactCorrupt);
    }
    let mut file = File::open(path).map_err(CacheInstallError::Io)?;
    let mut hash = Sha256::new();
    let mut total: u64 = 0;
    let mut buffer = vec![0u8; INSTALL_CHUNK_BYTES];
    loop {
        checked(check)?;
        let read = file.read(&mut buffer).map_err(CacheInstallError::Io)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or(CacheInstallError::ExistingArtifactCorrupt)?;
        if total > pin.bytes {
            return Err(CacheInstallError::ExistingArtifactCorrupt);
        }
        hash.update(&buffer[..read]);
        checked(check)?;
    }
    checked(check)?;
    if total != pin.bytes || format!("{:x}", hash.finalize()) != pin.sha256 {
        return Err(CacheInstallError::ExistingArtifactCorrupt);
    }
    Ok(())
}
#[cfg(unix)]
fn sync_parent_dir_best_effort(path: &Path) {
    if let Some(parent) = path.parent() {
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
    }
}
#[cfg(not(unix))]
fn sync_parent_dir_best_effort(_path: &Path) {}
struct TemporaryInstall {
    directory: PathBuf,
}
impl TemporaryInstall {
    fn create(cache_dir: &Path) -> Result<Self, CacheInstallError> {
        for _ in 0..TEMP_DIR_ATTEMPTS {
            let directory =
                cache_dir.join(format!("{TEMP_DIR_PREFIX}{:016x}", rand::random::<u64>()));
            match fs::create_dir(&directory) {
                Ok(()) => return Ok(Self { directory }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(CacheInstallError::Io(error)),
            }
        }
        Err(CacheInstallError::Io(io::Error::other(
            "could not allocate a unique temporary install directory",
        )))
    }
    fn file_path(&self) -> PathBuf {
        self.directory.join("artifact")
    }
    fn discard(self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
#[cfg(test)]
#[path = "local-embedding-cache-tests.rs"]
mod tests;
