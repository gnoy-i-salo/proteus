//! Bounded recovery of orphaned temporary model-artifact installs from the
//! host-owned pinned model cache.
//!
//! The verified installer in `local-embedding-cache.rs` streams each artifact
//! into an exclusive temporary directory `<cache-dir>/.artifact-install-` plus
//! exactly 16 lowercase hex characters, containing at most one regular file
//! named `artifact`, and hard-links the verified file into its
//! content-addressed destination. When the owning process dies mid-install,
//! that temporary directory can be left behind. This module removes only such
//! orphans: a temp directory that is either empty or holds exactly one regular
//! file named `artifact`. Published content-addressed files, unknown names,
//! nested trees and symlink targets are never touched.
//!
//! The service host calls [`cleanup_interrupted_installs`] under its exclusive
//! runtime maintenance gate before a new explicit download, with no concurrent
//! writer in this process. Cancellation is cooperative through `check`.
//!
//! Deliberate limitations:
//! - This offers no protection against a malicious same-user filesystem race:
//!   a same-user process can swap entries between validation and removal.
//! - This is not generic secure deletion: removed files are unlinked with the
//!   plain OS calls, their bytes are not overwritten.
//! - Cancellation may leave already-cleaned directories absent and the rest
//!   unchanged; there is no rollback promise.
//! - A failed or cancelled removal reports the error and leaves whatever was
//!   already removed removed.
use std::fs;
use std::io;
use std::path::{Component, Path};

/// Maximum number of direct cache-root entries enumerated per cleanup run.
pub const MAX_SCAN_ENTRIES: usize = 1024;
/// Maximum number of recognized temporary directories processed per cleanup
/// run.
pub const MAX_RECOGNIZED_TEMP_DIRS: usize = 128;

const TEMP_DIR_PREFIX: &str = ".artifact-install-";
const TEMP_DIR_SUFFIX_LEN: usize = 16;
const TEMP_ARTIFACT_NAME: &str = "artifact";

/// Bounded result of one cleanup run. `scanned` counts every direct cache-root
/// entry enumerated; `removed` counts temporary directories fully removed;
/// `skipped` counts entries left untouched, so `scanned == removed + skipped`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CleanupSummary {
    pub scanned: usize,
    pub removed: usize,
    pub skipped: usize,
}

/// Typed failure of a cleanup run. Variants carry no secrets.
#[derive(Debug)]
pub enum CleanupError {
    /// The cache root is not absolute or contains a parent component.
    InvalidRootPath,
    /// The cache root exists but is a symlink or not a directory.
    CacheDirectoryInvalid,
    /// The cooperative cancellation/deadline check aborted the cleanup; the
    /// reason is preserved exactly as the callback produced it.
    Cancelled(String),
    /// Direct root entries exceeded [`MAX_SCAN_ENTRIES`]; enumeration stopped
    /// before unbounded work.
    ScanLimitExceeded,
    /// Recognized temporary directories exceeded [`MAX_RECOGNIZED_TEMP_DIRS`];
    /// processing stopped before unbounded work.
    TempDirLimitExceeded,
    /// Other local filesystem failure.
    Io(io::Error),
}
impl std::fmt::Display for CleanupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRootPath => {
                write!(
                    formatter,
                    "cache root must be absolute with no parent components"
                )
            }
            Self::CacheDirectoryInvalid => {
                write!(formatter, "cache root is not a real directory")
            }
            Self::Cancelled(reason) => write!(formatter, "cleanup cancelled: {reason}"),
            Self::ScanLimitExceeded => write!(
                formatter,
                "cache scan exceeded the entry budget of {MAX_SCAN_ENTRIES}"
            ),
            Self::TempDirLimitExceeded => write!(
                formatter,
                "temporary directory budget of {MAX_RECOGNIZED_TEMP_DIRS} exceeded"
            ),
            Self::Io(error) => write!(formatter, "cache cleanup filesystem error: {error}"),
        }
    }
}
impl std::error::Error for CleanupError {}

/// Removes orphaned temporary install directories from `cache_dir`.
///
/// `check` is cooperative cancellation/deadline input owned by the caller. It
/// runs once before any cache IO, once per entry during root enumeration, once
/// before each removal and once before a successful return; a failure aborts
/// with [`CleanupError::Cancelled`] and the reason preserved exactly. A missing
/// cache root is a no-op that returns a zero summary and never creates the
/// root. The root must be absolute and free of parent components, and must be
/// a real directory: a symlinked or non-directory root is rejected. Work is
/// bounded by [`MAX_SCAN_ENTRIES`] and [`MAX_RECOGNIZED_TEMP_DIRS`]; exceeding
/// either cap returns an explicit error without unbounded work. Only the exact
/// owned temporary name `.artifact-install-` followed by exactly 16 lowercase
/// hex characters is eligible, and only when it is a directory holding zero
/// children or exactly one regular file named `artifact`; the file is removed
/// first, then the empty directory, with no recursive deletion. Everything
/// else is left untouched and reported as skipped.
pub fn cleanup_interrupted_installs(
    cache_dir: &Path,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<CleanupSummary, CleanupError> {
    checked(check)?;
    validate_root_path(cache_dir)?;
    match fs::symlink_metadata(cache_dir) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(CleanupError::CacheDirectoryInvalid);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            checked(check)?;
            return Ok(CleanupSummary::default());
        }
        Err(error) => return Err(CleanupError::Io(error)),
    }
    let mut summary = CleanupSummary::default();
    let mut recognized = 0usize;
    for item in fs::read_dir(cache_dir).map_err(CleanupError::Io)? {
        checked(check)?;
        let entry = item.map_err(CleanupError::Io)?;
        summary.scanned += 1;
        if summary.scanned > MAX_SCAN_ENTRIES {
            return Err(CleanupError::ScanLimitExceeded);
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            summary.skipped += 1;
            continue;
        };
        if !is_owned_temp_name(name) {
            summary.skipped += 1;
            continue;
        }
        recognized += 1;
        if recognized > MAX_RECOGNIZED_TEMP_DIRS {
            return Err(CleanupError::TempDirLimitExceeded);
        }
        if remove_if_eligible(&entry.path(), check)? {
            summary.removed += 1;
        } else {
            summary.skipped += 1;
        }
    }
    checked(check)?;
    Ok(summary)
}
/// Removes `path` when it is an eligible orphaned temporary install directory:
/// a real directory holding zero children or exactly one regular file named
/// `artifact`. Returns whether the directory was removed. Symlinks, non-
/// directories, vanished entries and any unexpected content are skipped, and
/// removal is strictly `remove_file` of `artifact` followed by `remove_dir` of
/// the then-empty directory — never a recursive delete.
fn remove_if_eligible(
    path: &Path,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<bool, CleanupError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(CleanupError::Io(error)),
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() || !file_type.is_dir() {
        return Ok(false);
    }
    let mut children = fs::read_dir(path).map_err(CleanupError::Io)?;
    let has_artifact = match children.next() {
        None => false,
        Some(first) => {
            let first = first.map_err(CleanupError::Io)?;
            let first_name = first.file_name();
            let name_matches = first_name
                .to_str()
                .is_some_and(|name| name == TEMP_ARTIFACT_NAME);
            if !name_matches {
                return Ok(false);
            }
            let first_metadata = fs::symlink_metadata(first.path()).map_err(CleanupError::Io)?;
            let first_type = first_metadata.file_type();
            if first_type.is_symlink() || !first_type.is_file() {
                return Ok(false);
            }
            true
        }
    };
    if children
        .next()
        .transpose()
        .map_err(CleanupError::Io)?
        .is_some()
    {
        return Ok(false);
    }
    // Close enumeration before removing its directory, including on Windows.
    drop(children);
    checked(check)?;
    if has_artifact {
        fs::remove_file(path.join(TEMP_ARTIFACT_NAME)).map_err(CleanupError::Io)?;
        checked(check)?;
    }
    fs::remove_dir(path).map_err(CleanupError::Io)?;
    Ok(true)
}
fn checked(check: &dyn Fn() -> Result<(), String>) -> Result<(), CleanupError> {
    check().map_err(CleanupError::Cancelled)
}
fn validate_root_path(cache_dir: &Path) -> Result<(), CleanupError> {
    let absolute = cache_dir.is_absolute();
    let has_parent = cache_dir
        .components()
        .any(|component| matches!(component, Component::ParentDir));
    if !absolute || has_parent {
        return Err(CleanupError::InvalidRootPath);
    }
    Ok(())
}
fn is_owned_temp_name(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix(TEMP_DIR_PREFIX) else {
        return false;
    };
    suffix.len() == TEMP_DIR_SUFFIX_LEN
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
#[cfg(test)]
#[path = "local-model-cleanup-tests.rs"]
mod tests;
