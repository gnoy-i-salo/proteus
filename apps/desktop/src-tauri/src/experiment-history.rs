//! Bounded, read-only access to the host-owned local experiment history.
//!
//! Each saved run lives in `<base>/<id>/result.json`, where `<base>` is the
//! experiments directory chosen by the service host (desktop
//! `app_data_dir/experiments`) and `<id>` is the opaque run id previously
//! accepted by `save_experiment`. This module only reads existing
//! `result.json` files: it never resolves a caller-supplied path, never
//! touches person artifacts, never rewrites legacy JSON, recomputes scores or
//! invokes CLIs. Historical schema decoding is intentionally left to the
//! TypeScript contracts layer; the only structural requirement here is a JSON
//! object envelope.
//!
//! Hard bounds: a directory scan examines at most [`MAX_SCAN_ENTRIES`] direct
//! entries; a page returns at most [`MAX_PAGE_SIZE`] summaries; one result
//! read accepts at most [`MAX_RESULT_BYTES`] (matching the native save cap)
//! and preserves the exact bytes (UTF-8, CRLF and whitespace intact).
//!
//! Detected symlinked base/run directories, symlinked `result.json` and
//! non-file results are rejected. Checks use `symlink_metadata` plus a
//! no-link path re-check after opening; this is not atomic path traversal.
//!
//! Deliberate limitations:
//! - TOCTOU: a same-user process can swap entries between the metadata check
//!   and the open. These metadata checks do not provide descriptor-relative
//!   no-follow traversal or guarantee file identity under concurrent mutation.
//! - Listing validates structure only (run directory, `result.json` presence,
//!   type and size). Damaged entries keep their id and carry a stable error
//!   token so one bad entry never hides valid ones; content-level damage is
//!   reported by [`read_saved_run`], never by the listing, which must not load
//!   every run text.
//! - A missing base directory is an empty page, not an error; genuine root
//!   I/O failures (unreadable base, `result.json` occupied by a directory,
//!   oversized file at read time) fail explicitly.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use serde::Serialize;

use crate::cli::model::valid_id;

/// Maximum accepted `result.json` size in bytes; matches the native save cap.
pub const MAX_RESULT_BYTES: u64 = 8 * 1024 * 1024;
/// Maximum number of direct base-directory entries examined per listing. When
/// more entries exist, the scan stops and the page is marked truncated.
pub const MAX_SCAN_ENTRIES: usize = 1024;
/// Maximum number of summaries returned in one page.
pub const MAX_PAGE_SIZE: usize = 100;
/// Page size used when the caller passes `limit == 0`.
pub const DEFAULT_PAGE_SIZE: usize = 50;

const RESULT_FILE_NAME: &str = "result.json";

/// One saved-run entry of a listing page. `id` is the opaque run id; `ok` is
/// `false` when the entry is structurally damaged, with a stable machine
/// error token in `error` (for example `missingResult`, `symlinkedRun`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedRunSummary {
    pub id: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// One page of the scanned saved-run subset, sorted by run id in byte
/// order. `matched` counts the conforming runs seen within the scan cap;
/// `next_offset` is `Some` only when a further page exists; `scan_truncated`
/// signals that the scan stopped at [`MAX_SCAN_ENTRIES`] entries, in which
/// case `matched` is a lower bound.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedRunPage {
    pub schema_version: u8,
    pub entries: Vec<SavedRunSummary>,
    pub matched: usize,
    pub next_offset: Option<usize>,
    pub scan_truncated: bool,
}

/// Exact bytes of one `result.json`, validated as UTF-8 text and a JSON
/// object envelope. Bytes are preserved exactly (CRLF, whitespace, Unicode).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedRunText {
    pub schema_version: u8,
    pub id: String,
    pub text: String,
}

/// Verifies the host-owned base directory. Returns `Ok(false)` when it does
/// not exist (an empty history), `Ok(true)` when it is a real directory, and
/// `Err` for a symlinked, non-directory or unreadable base.
fn ensure_base(base: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(base) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("Experiments directory is unreadable.".into()),
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                Err("Experiments directory is unavailable.".into())
            } else {
                Ok(true)
            }
        }
    }
}

enum RunInspection {
    /// Not a run directory (a stray file and the like): excluded silently.
    Skip,
    Ready,
    /// Structurally damaged; carries a stable machine error token.
    Damaged(&'static str),
}

fn inspect_run(run_dir: &Path) -> RunInspection {
    let metadata = match fs::symlink_metadata(run_dir) {
        Ok(metadata) => metadata,
        Err(_) => return RunInspection::Damaged("unreadableRun"),
    };
    if metadata.file_type().is_symlink() {
        return RunInspection::Damaged("symlinkedRun");
    }
    if !metadata.is_dir() {
        return RunInspection::Skip;
    }
    match inspect_result_file(&run_dir.join(RESULT_FILE_NAME)) {
        Ok(()) => RunInspection::Ready,
        Err(token) => RunInspection::Damaged(token),
    }
}

fn inspect_result_file(path: &Path) -> Result<(), &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            "missingResult"
        } else {
            "unreadableResult"
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err("symlinkedResult");
    }
    if !metadata.is_file() {
        return Err("notAFile");
    }
    if metadata.len() > MAX_RESULT_BYTES {
        return Err("oversizeResult");
    }
    Ok(())
}

/// Scans at most `max_scan` direct base entries and summarizes every
/// conforming run id found. Returns the summaries (unsorted) and whether the
/// scan stopped at the cap.
fn collect_summaries(base: &Path, max_scan: usize) -> Result<(Vec<SavedRunSummary>, bool), String> {
    let mut summaries = Vec::new();
    let mut truncated = false;
    let entries = fs::read_dir(base).map_err(|_| "Experiments directory is unreadable.")?;
    for (index, item) in entries.enumerate() {
        if index >= max_scan {
            truncated = true;
            break;
        }
        let item = item.map_err(|_| "Experiments directory is unreadable.")?;
        let name = item.file_name();
        let Some(id) = name.to_str() else {
            continue;
        };
        if !valid_id(id) {
            continue;
        }
        let summary = match inspect_run(&base.join(id)) {
            RunInspection::Skip => continue,
            RunInspection::Ready => SavedRunSummary {
                id: id.to_string(),
                ok: true,
                error: None,
            },
            RunInspection::Damaged(token) => SavedRunSummary {
                id: id.to_string(),
                ok: false,
                error: Some(token.to_string()),
            },
        };
        summaries.push(summary);
    }
    Ok((summaries, truncated))
}

/// Sorts the scanned subset under `base` by byte-order of run id.
/// A truncated scan is not a stable snapshot across requests; callers must
/// disclose partial coverage and cannot infer gap-free history pagination.
/// `offset` skips that many entries; `limit` is clamped to
/// [`MAX_PAGE_SIZE`] with [`DEFAULT_PAGE_SIZE`] used when `0`. A missing base
/// is an empty page; a present but unreadable base fails explicitly.
pub fn list_saved_runs(base: &Path, offset: usize, limit: usize) -> Result<SavedRunPage, String> {
    if !ensure_base(base)? {
        return Ok(SavedRunPage {
            schema_version: 1,
            entries: Vec::new(),
            matched: 0,
            next_offset: None,
            scan_truncated: false,
        });
    }
    let (mut summaries, scan_truncated) = collect_summaries(base, MAX_SCAN_ENTRIES)?;
    summaries.sort_by(|a, b| a.id.cmp(&b.id));
    let matched = summaries.len();
    let limit = if limit == 0 {
        DEFAULT_PAGE_SIZE
    } else {
        limit.min(MAX_PAGE_SIZE)
    };
    let entries: Vec<SavedRunSummary> = summaries.into_iter().skip(offset).take(limit).collect();
    let consumed = offset.saturating_add(entries.len());
    let next_offset = (consumed < matched).then_some(consumed);
    Ok(SavedRunPage {
        schema_version: 1,
        entries,
        matched,
        next_offset,
        scan_truncated,
    })
}

/// Reads one saved run's exact `result.json` text. The id is validated with
/// the shared `valid_id` charset before any path is joined, rejecting caller
/// path-component injection. Detected symlinks and non-files are rejected;
/// same-user filesystem races remain outside the guarantee. The
/// content must be UTF-8 text within [`MAX_RESULT_BYTES`] forming a JSON
/// object envelope; the full historical schema decode belongs to the
/// TypeScript contracts layer.
pub fn read_saved_run(base: &Path, id: &str) -> Result<SavedRunText, String> {
    if !valid_id(id) {
        return Err("Invalid saved run id.".into());
    }
    // A symlinked or non-directory base fails explicitly; a missing base falls
    // through to the not-found error below.
    ensure_base(base)?;
    let run_dir = base.join(id);
    let metadata = fs::symlink_metadata(&run_dir).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            "Saved run not found."
        } else {
            "Saved run is unreadable."
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err("Saved run directory is a symlink.".into());
    }
    if !metadata.is_dir() {
        return Err("Saved run is not a directory.".into());
    }
    let path = run_dir.join(RESULT_FILE_NAME);
    inspect_result_file(&path).map_err(|token| {
        let message = match token {
            "missingResult" => "Saved run result is missing.",
            "symlinkedResult" => "Saved run result is a symlink.",
            "notAFile" => "Saved run result is not a file.",
            "oversizeResult" => "Saved run result exceeds the 8 MiB limit.",
            _ => "Saved run result is unreadable.",
        };
        message.to_string()
    })?;
    let file = File::open(&path).map_err(|_| "Saved run result is unreadable.")?;
    let descriptor_metadata = file
        .metadata()
        .map_err(|_| "Saved run result is unreadable.")?;
    if !descriptor_metadata.is_file() || descriptor_metadata.len() > MAX_RESULT_BYTES {
        return Err("Saved run result changed while reading.".into());
    }
    let mut text = String::new();
    file.take(MAX_RESULT_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::InvalidData {
                "Saved run result is not valid UTF-8."
            } else {
                "Saved run result is unreadable."
            }
        })?;
    if text.len() as u64 > MAX_RESULT_BYTES {
        return Err("Saved run result exceeds the 8 MiB limit.".into());
    }
    // TOCTOU re-check: between the first check and the open the path may have
    // been swapped for a symlink or another non-file. The descriptor itself
    // passed the checks. Re-examine the path to reject detectable swaps;
    // this does not compare file identity or exclude all same-user races.
    let rechecked = fs::symlink_metadata(&path).map_err(|_| "Saved run result is unreadable.")?;
    if rechecked.file_type().is_symlink() || !rechecked.is_file() {
        return Err("Saved run result changed while reading.".into());
    }
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "Saved run result is not valid JSON.")?;
    if !value.is_object() {
        return Err("Saved run result is not a JSON object.".into());
    }
    Ok(SavedRunText {
        schema_version: 1,
        id: id.to_string(),
        text,
    })
}

#[cfg(test)]
#[path = "experiment-history-tests.rs"]
mod tests;

#[path = "experiment-history-commands.rs"]
pub mod commands;
