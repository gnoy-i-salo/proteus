//! Isolated std-only tests for orphan temporary-install cleanup.
//!
//! Fixtures are deterministic: unique RAII temporary directories, fixed byte
//! payloads, and sorted-name comparisons. Cross-platform tests use only std;
//! symlink and socket cases are guarded with `#[cfg(unix)]`. Cooperative
//! cancellation is scripted by call counters, never by sleeps, and no test
//! relies on directory iteration order.
use super::*;
use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIR_ID: AtomicUsize = AtomicUsize::new(0);

/// RAII unique temporary directory, removed on drop.
struct TestDir(PathBuf);
impl TestDir {
    fn new(label: &str) -> Self {
        let id = NEXT_DIR_ID.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "proteus-cleanup-test-{}-{id}-{label}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create test dir");
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Exact owned temporary directory name for a seed value.
fn temp_name(seed: u64) -> String {
    format!(".artifact-install-{seed:016x}")
}
/// Published content-addressed artifact name (64 lowercase hex chars).
fn published_name(seed: u64) -> String {
    format!("{seed:064x}")
}
/// Sorted direct entry names of `path`, for order-independent assertions.
fn entry_names(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(path)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}
fn write_artifact(dir: &Path, bytes: &[u8]) {
    fs::write(dir.join(TEMP_ARTIFACT_NAME), bytes).expect("write artifact");
}
/// Creates an eligible temporary directory with one regular `artifact` file.
fn seed_temp_with_artifact(root: &Path, seed: u64, bytes: &[u8]) -> PathBuf {
    let dir = root.join(temp_name(seed));
    fs::create_dir(&dir).expect("create temp dir");
    write_artifact(&dir, bytes);
    dir
}
/// Creates an eligible empty temporary directory.
fn seed_empty_temp(root: &Path, seed: u64) -> PathBuf {
    let dir = root.join(temp_name(seed));
    fs::create_dir(&dir).expect("create empty temp dir");
    dir
}

/// Scripted cooperative check: counts calls and fails with `reason` once the
/// call count exceeds `fail_after`. With `fail_after == usize::MAX` it never
/// fails.
struct ScriptedCheck {
    calls: Cell<usize>,
    fail_after: usize,
    reason: &'static str,
}
impl ScriptedCheck {
    fn passing() -> Self {
        Self {
            calls: Cell::new(0),
            fail_after: usize::MAX,
            reason: "",
        }
    }
    fn fail_after(fail_after: usize, reason: &'static str) -> Self {
        Self {
            calls: Cell::new(0),
            fail_after,
            reason,
        }
    }
    fn check(&self) -> Result<(), String> {
        self.calls.set(self.calls.get() + 1);
        if self.calls.get() > self.fail_after {
            return Err(self.reason.to_string());
        }
        Ok(())
    }
    fn call_count(&self) -> usize {
        self.calls.get()
    }
}
fn cancelled_reason(result: Result<CleanupSummary, CleanupError>) -> String {
    match result {
        Err(CleanupError::Cancelled(reason)) => reason,
        other => panic!("expected Cancelled, got {other:?}"),
    }
}
/// Total `check` calls for a cache holding `eligible` eligible temporary
/// directories and nothing else: one initial, one enumeration check and one
/// pre-file and pre-directory removal checks per entry, one before success.
fn expected_calls_for(eligible: usize) -> usize {
    1 + eligible * 3 + 1
}

#[test]
fn missing_cache_root_is_noop_and_not_created() {
    let base = TestDir::new("missing");
    let root = base.path().join("absent-cache");
    let check = ScriptedCheck::passing();
    let summary =
        cleanup_interrupted_installs(&root, &|| check.check()).expect("missing root no-op");
    assert_eq!(summary, CleanupSummary::default());
    assert!(!root.exists(), "missing root must not be created");
    assert_eq!(
        check.call_count(),
        2,
        "checks precede IO and successful return"
    );
}

#[test]
fn empty_cache_returns_zero_summary() {
    let base = TestDir::new("empty");
    let summary = cleanup_interrupted_installs(base.path(), &|| Ok(())).expect("empty cleanup");
    assert_eq!(summary, CleanupSummary::default());
}

#[test]
fn missing_cache_still_checks_cancellation_before_success() {
    let base = TestDir::new("missing-final-check");
    let check = ScriptedCheck::fail_after(1, "late cancellation");
    let result = cleanup_interrupted_installs(&base.path().join("absent"), &|| check.check());
    assert_eq!(cancelled_reason(result), "late cancellation");
}

#[test]
fn cancellation_between_file_and_directory_removal_leaves_empty_directory_for_retry() {
    let base = TestDir::new("between-removals");
    let temporary = seed_temp_with_artifact(base.path(), 1, b"partial");
    let check = || {
        if !temporary.join("artifact").exists() {
            Err("stop after unlink".into())
        } else {
            Ok(())
        }
    };
    assert_eq!(
        cancelled_reason(cleanup_interrupted_installs(base.path(), &check)),
        "stop after unlink"
    );
    assert!(temporary.is_dir());
    assert!(entry_names(&temporary).is_empty());
    assert_eq!(
        cleanup_interrupted_installs(base.path(), &|| Ok(()))
            .unwrap()
            .removed,
        1
    );
}

#[test]
fn relative_root_rejected_before_any_io() {
    let check = ScriptedCheck::passing();
    let result = cleanup_interrupted_installs(Path::new("relative/cache"), &|| check.check());
    assert!(matches!(result, Err(CleanupError::InvalidRootPath)));
}

#[test]
fn parent_component_root_rejected_before_any_io() {
    let check = ScriptedCheck::passing();
    let result = cleanup_interrupted_installs(Path::new("/tmp/../var/cache"), &|| check.check());
    assert!(matches!(result, Err(CleanupError::InvalidRootPath)));
}

#[test]
fn regular_file_root_rejected() {
    let base = TestDir::new("file-root");
    let root = base.path().join("not-a-dir");
    fs::write(&root, b"plain bytes").expect("seed regular file root");
    let result = cleanup_interrupted_installs(&root, &|| Ok(()));
    assert!(matches!(result, Err(CleanupError::CacheDirectoryInvalid)));
    assert!(root.is_file(), "regular file root must be untouched");
}

#[test]
fn removes_empty_temp_dir() {
    let base = TestDir::new("empty-temp");
    let temp = seed_empty_temp(base.path(), 7);
    let summary = cleanup_interrupted_installs(base.path(), &|| Ok(())).expect("cleanup");
    assert_eq!(summary.scanned, 1);
    assert_eq!(summary.removed, 1);
    assert_eq!(summary.skipped, 0);
    assert!(!temp.exists(), "empty temp dir must be removed");
}

#[test]
fn removes_temp_dir_with_single_regular_artifact() {
    let base = TestDir::new("artifact-temp");
    let bytes = b"partial artifact bytes".to_vec();
    let temp = seed_temp_with_artifact(base.path(), 9, &bytes);
    let summary = cleanup_interrupted_installs(base.path(), &|| Ok(())).expect("cleanup");
    assert_eq!(summary.removed, 1);
    assert!(!temp.exists(), "temp dir with artifact must be removed");
    assert_eq!(entry_names(base.path()), Vec::<String>::new());
}

#[test]
fn published_content_addressed_files_are_preserved() {
    let base = TestDir::new("published");
    let first = published_name(0xABC);
    let second = published_name(0x123);
    let first_bytes = b"verified weights".to_vec();
    let second_bytes = b"verified config".to_vec();
    fs::write(base.path().join(&first), &first_bytes).expect("seed published");
    fs::write(base.path().join(&second), &second_bytes).expect("seed published");
    seed_temp_with_artifact(base.path(), 3, b"orphan");
    let summary = cleanup_interrupted_installs(base.path(), &|| Ok(())).expect("cleanup");
    assert_eq!(summary.scanned, 3);
    assert_eq!(summary.removed, 1);
    assert_eq!(
        summary.skipped, 2,
        "published files are skipped, never deleted"
    );
    assert_eq!(
        fs::read(base.path().join(&first)).expect("first preserved"),
        first_bytes
    );
    assert_eq!(
        fs::read(base.path().join(&second)).expect("second preserved"),
        second_bytes
    );
}

#[test]
fn near_miss_names_are_skipped() {
    let base = TestDir::new("near-miss");
    // 15 hex chars: too short.
    fs::create_dir(base.path().join(".artifact-install-0123456789abcde")).expect("seed 15-hex");
    // 17 hex chars: too long.
    fs::create_dir(base.path().join(".artifact-install-0123456789abcdef0")).expect("seed 17-hex");
    // Uppercase hex is not the owned name.
    fs::create_dir(base.path().join(".artifact-install-0123456789ABCDEF")).expect("seed upper");
    // Missing suffix entirely.
    fs::create_dir(base.path().join(".artifact-install-")).expect("seed bare prefix");
    // Different prefix.
    fs::create_dir(base.path().join(".other-install-0123456789abcdef")).expect("seed other");
    // Exact name but a regular file, not a directory.
    fs::write(
        base.path().join(temp_name(42)),
        b"stray file with temp name",
    )
    .expect("seed temp-named file");
    let before = entry_names(base.path());
    let summary = cleanup_interrupted_installs(base.path(), &|| Ok(())).expect("cleanup");
    assert_eq!(summary.removed, 0);
    assert_eq!(summary.skipped, before.len());
    assert_eq!(summary.scanned, before.len());
    assert_eq!(
        entry_names(base.path()),
        before,
        "every near-miss name must be retained"
    );
}

#[test]
fn temp_dir_with_subdirectory_is_skipped_untouched() {
    let base = TestDir::new("nested-temp");
    let temp = base.path().join(temp_name(11));
    fs::create_dir_all(temp.join("nested").join("deep")).expect("seed nested tree");
    fs::write(temp.join("nested").join("deep").join("payload"), b"data").expect("seed payload");
    let summary = cleanup_interrupted_installs(base.path(), &|| Ok(())).expect("cleanup");
    assert_eq!(summary.removed, 0);
    assert_eq!(summary.skipped, 1);
    assert!(
        temp.join("nested").join("deep").join("payload").is_file(),
        "nested trees must never be recursively removed"
    );
}

#[test]
fn temp_dir_with_extra_file_is_skipped_untouched() {
    let base = TestDir::new("extra-file");
    let temp = seed_temp_with_artifact(base.path(), 13, b"partial");
    fs::write(temp.join("notes"), b"unexpected second entry").expect("seed extra file");
    let summary = cleanup_interrupted_installs(base.path(), &|| Ok(())).expect("cleanup");
    assert_eq!(summary.removed, 0);
    assert_eq!(summary.skipped, 1);
    assert_eq!(
        fs::read(temp.join(TEMP_ARTIFACT_NAME)).expect("artifact kept"),
        b"partial"
    );
    assert!(temp.join("notes").is_file(), "extra entry must be retained");
}

#[test]
fn temp_dir_with_unknown_single_file_is_skipped() {
    let base = TestDir::new("unknown-file");
    let temp = base.path().join(temp_name(17));
    fs::create_dir(&temp).expect("create temp dir");
    fs::write(temp.join("mystery.bin"), b"?").expect("seed unknown file");
    let summary = cleanup_interrupted_installs(base.path(), &|| Ok(())).expect("cleanup");
    assert_eq!(summary.removed, 0);
    assert_eq!(summary.skipped, 1);
    assert!(temp.is_dir(), "dir with unknown file name must be retained");
    assert_eq!(fs::read(temp.join("mystery.bin")).expect("file kept"), b"?");
}

#[test]
fn temp_dir_with_artifact_subdirectory_is_skipped() {
    let base = TestDir::new("artifact-dir");
    let temp = base.path().join(temp_name(19));
    fs::create_dir_all(temp.join(TEMP_ARTIFACT_NAME)).expect("artifact as directory");
    let summary = cleanup_interrupted_installs(base.path(), &|| Ok(())).expect("cleanup");
    assert_eq!(summary.removed, 0);
    assert!(
        temp.join(TEMP_ARTIFACT_NAME).is_dir(),
        "artifact directory must be retained"
    );
}

#[cfg(unix)]
#[test]
fn symlink_root_rejected_and_target_untouched() {
    use std::os::unix::fs::symlink;
    let base = TestDir::new("symlink-root");
    let target = base.path().join("real-cache");
    fs::create_dir(&target).expect("create real dir");
    let payload = target.join(published_name(1));
    fs::write(&payload, b"payload").expect("seed payload");
    let link = base.path().join("linked-cache");
    symlink(&target, &link).expect("create symlink root");
    let result = cleanup_interrupted_installs(&link, &|| Ok(()));
    assert!(matches!(result, Err(CleanupError::CacheDirectoryInvalid)));
    assert!(
        payload.is_file(),
        "symlink target content must be untouched"
    );
}

#[cfg(unix)]
#[test]
fn temp_dir_symlink_is_not_followed() {
    use std::os::unix::fs::symlink;
    let base = TestDir::new("symlink-temp");
    let root = base.path().join("cache");
    fs::create_dir(&root).expect("create cache root");
    let outside = base.path().join("outside");
    fs::create_dir(&outside).expect("create outside dir");
    let keeper = outside.join("keeper.bin");
    fs::write(&keeper, b"keeper bytes").expect("seed outside file");
    let link = root.join(temp_name(23));
    symlink(&outside, &link).expect("create temp-named symlink");
    let summary = cleanup_interrupted_installs(&root, &|| Ok(())).expect("cleanup");
    assert_eq!(summary.removed, 0);
    assert_eq!(summary.skipped, 1, "symlinked temp dir is skipped");
    assert!(link
        .symlink_metadata()
        .expect("link stat")
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(&keeper).expect("target kept"), b"keeper bytes");
    assert!(outside.is_dir(), "symlink target directory must remain");
}

#[cfg(unix)]
#[test]
fn artifact_symlink_is_not_followed_and_target_survives() {
    use std::os::unix::fs::symlink;
    let base = TestDir::new("artifact-symlink");
    let root = base.path().join("cache");
    fs::create_dir(&root).expect("create cache root");
    let target = base.path().join("precious.bin");
    fs::write(&target, b"precious bytes").expect("seed symlink target");
    let temp = root.join(temp_name(29));
    fs::create_dir(&temp).expect("create temp dir");
    symlink(&target, temp.join(TEMP_ARTIFACT_NAME)).expect("create artifact symlink");
    let summary = cleanup_interrupted_installs(&root, &|| Ok(())).expect("cleanup");
    assert_eq!(summary.removed, 0);
    assert_eq!(summary.skipped, 1);
    assert_eq!(
        fs::read(&target).expect("symlink target preserved"),
        b"precious bytes",
        "symlink target must never be deleted"
    );
    assert!(temp.is_dir(), "temp dir holding a symlink must be retained");
}

#[cfg(unix)]
#[test]
fn artifact_socket_is_skipped() {
    use std::os::unix::net::UnixListener;
    // Unix socket paths are length-capped (SUN_LEN), so this test roots the
    // cache directly under the short `/tmp` prefix instead of the deeper
    // per-user temp directory.
    let short = TestDir(Path::new("/tmp").join(format!(
        "pcs-{}-{}",
        std::process::id(),
        NEXT_DIR_ID.fetch_add(1, Ordering::SeqCst)
    )));
    fs::create_dir(short.path()).expect("create socket test dir");
    let temp = short.path().join(temp_name(31));
    fs::create_dir(&temp).expect("create temp dir");
    let socket = temp.join(TEMP_ARTIFACT_NAME);
    let _listener = UnixListener::bind(&socket).expect("bind unix socket");
    let summary = cleanup_interrupted_installs(short.path(), &|| Ok(())).expect("cleanup");
    assert_eq!(summary.removed, 0);
    assert_eq!(summary.skipped, 1);
    assert!(socket.exists(), "socket must be retained, never removed");
}

#[test]
fn cancelled_before_any_io_preserves_everything() {
    let base = TestDir::new("cancel-first");
    seed_temp_with_artifact(base.path(), 37, b"orphan");
    // The root itself is a regular file: any IO would report
    // CacheDirectoryInvalid, so observing Cancelled proves the check ran
    // before the first filesystem probe.
    let file_root = base.path().join("file-root");
    fs::write(&file_root, b"not a directory").expect("seed file root");
    let check = ScriptedCheck::fail_after(0, "maintenance window closed");
    let result = cleanup_interrupted_installs(&file_root, &|| check.check());
    assert_eq!(cancelled_reason(result), "maintenance window closed");
    assert_eq!(check.call_count(), 1);
    assert!(file_root.is_file(), "no IO may touch the root");
    assert!(
        base.path().join(temp_name(37)).is_dir(),
        "entries untouched"
    );
}

/// State-scripted check: fails once the number of owned temporary directories
/// in `root` drops below `initial`, i.e. immediately after the first removal.
/// This makes mid-cleanup cancellation deterministic regardless of directory
/// iteration order.
struct CancelAfterFirstRemoval<'a> {
    root: &'a Path,
    initial: usize,
    reason: &'static str,
}
impl CancelAfterFirstRemoval<'_> {
    fn check(&self) -> Result<(), String> {
        let remaining = fs::read_dir(self.root)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_name().to_str().is_some_and(is_owned_temp_name))
                    .count()
            })
            .unwrap_or(0);
        if remaining < self.initial {
            return Err(self.reason.to_string());
        }
        Ok(())
    }
}

#[test]
fn cancelled_mid_cleanup_keeps_remaining_dirs_intact() {
    let base = TestDir::new("cancel-mid");
    let published = published_name(5);
    fs::write(base.path().join(&published), b"verified").expect("seed published");
    for seed in 0..3u64 {
        seed_temp_with_artifact(base.path(), seed, format!("artifact {seed}").as_bytes());
    }
    let check = CancelAfterFirstRemoval {
        root: base.path(),
        initial: 3,
        reason: "budget revoked",
    };
    let result = cleanup_interrupted_installs(base.path(), &|| check.check());
    assert_eq!(cancelled_reason(result), "budget revoked");
    let remaining: Vec<PathBuf> = (0..3u64)
        .map(|seed| base.path().join(temp_name(seed)))
        .filter(|path| path.exists())
        .collect();
    assert_eq!(remaining.len(), 2, "exactly one eligible dir was removed");
    for path in &remaining {
        let seed_bytes =
            fs::read(path.join(TEMP_ARTIFACT_NAME)).expect("remaining artifact intact");
        let seed = path
            .file_name()
            .expect("name")
            .to_string_lossy()
            .strip_prefix(".artifact-install-")
            .expect("owned prefix")
            .to_string();
        let seed = u64::from_str_radix(&seed, 16).expect("hex seed");
        assert_eq!(
            seed_bytes,
            format!("artifact {seed}").into_bytes(),
            "each remaining dir fully intact"
        );
    }
    assert!(
        base.path().join(&published).is_file(),
        "published file survives cancellation"
    );
}

#[test]
fn cancelled_before_success_returns_cancelled_with_dirs_already_removed() {
    let base = TestDir::new("cancel-success");
    for seed in 0..2u64 {
        seed_temp_with_artifact(base.path(), seed, b"orphan");
    }
    // Every removal succeeds; the final pre-success check fails.
    let check = ScriptedCheck::fail_after(expected_calls_for(2) - 1, "deadline reached");
    let result = cleanup_interrupted_installs(base.path(), &|| check.check());
    assert_eq!(cancelled_reason(result), "deadline reached");
    assert_eq!(check.call_count(), expected_calls_for(2));
    assert_eq!(
        entry_names(base.path()),
        Vec::<String>::new(),
        "both dirs already removed"
    );
}

#[test]
fn scan_budget_allows_exactly_the_cap() {
    let base = TestDir::new("scan-cap-ok");
    for index in 0..MAX_SCAN_ENTRIES {
        fs::write(base.path().join(format!("plain-{index:04}")), b"x").expect("seed entry");
    }
    let summary = cleanup_interrupted_installs(base.path(), &|| Ok(())).expect("cleanup at cap");
    assert_eq!(summary.scanned, MAX_SCAN_ENTRIES);
    assert_eq!(summary.skipped, MAX_SCAN_ENTRIES);
    assert_eq!(summary.removed, 0);
}

#[test]
fn scan_budget_rejects_entry_beyond_cap() {
    let base = TestDir::new("scan-cap-exceeded");
    for index in 0..=MAX_SCAN_ENTRIES {
        fs::write(base.path().join(format!("plain-{index:04}")), b"x").expect("seed entry");
    }
    let result = cleanup_interrupted_installs(base.path(), &|| Ok(()));
    assert!(matches!(result, Err(CleanupError::ScanLimitExceeded)));
    assert_eq!(
        entry_names(base.path()).len(),
        MAX_SCAN_ENTRIES + 1,
        "no entry may be deleted on budget error"
    );
}

#[test]
fn temp_dir_budget_allows_exactly_the_cap() {
    let base = TestDir::new("temp-cap-ok");
    for seed in 0..MAX_RECOGNIZED_TEMP_DIRS as u64 {
        seed_temp_with_artifact(base.path(), seed, b"orphan");
    }
    let summary = cleanup_interrupted_installs(base.path(), &|| Ok(())).expect("cleanup at cap");
    assert_eq!(summary.scanned, MAX_RECOGNIZED_TEMP_DIRS);
    assert_eq!(summary.removed, MAX_RECOGNIZED_TEMP_DIRS);
    assert_eq!(summary.skipped, 0);
    assert_eq!(entry_names(base.path()), Vec::<String>::new());
}

#[test]
fn temp_dir_budget_rejects_dir_beyond_cap() {
    let base = TestDir::new("temp-cap-exceeded");
    for seed in 0..=MAX_RECOGNIZED_TEMP_DIRS as u64 {
        seed_temp_with_artifact(base.path(), seed, b"orphan");
    }
    let result = cleanup_interrupted_installs(base.path(), &|| Ok(()));
    assert!(matches!(result, Err(CleanupError::TempDirLimitExceeded)));
    let remaining = entry_names(base.path());
    assert!(
        !remaining.is_empty(),
        "the cap must abort before exhausting entries"
    );
    for name in remaining {
        let dir = base.path().join(&name);
        assert!(dir.is_dir(), "every remaining entry is an untouched dir");
        assert!(
            dir.join(TEMP_ARTIFACT_NAME).is_file(),
            "remaining artifact intact"
        );
    }
}
