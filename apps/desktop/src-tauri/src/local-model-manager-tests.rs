use super::*;
use crate::local_embedding::cache::INSTALL_CHUNK_BYTES;
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn pin_for(bytes: &[u8]) -> ArtifactPin {
    ArtifactPin::new(bytes.len() as u64, hex_digest(bytes)).expect("valid pin")
}
/// Three deterministic synthetic artifacts with distinct bytes and sizes.
struct SyntheticPackage {
    weights: Vec<u8>,
    config: Vec<u8>,
    tokenizer: Vec<u8>,
    descriptor: ModelPackageDescriptor,
}
fn synthetic_package() -> SyntheticPackage {
    let weights: Vec<u8> = (0u32..100_000).map(|index| (index % 253) as u8).collect();
    let config: Vec<u8> = (0u32..512).map(|index| (index % 251) as u8).collect();
    let tokenizer: Vec<u8> = (0u32..2048).map(|index| (index % 249) as u8).collect();
    let descriptor = ModelPackageDescriptor {
        weights: pin_for(&weights),
        config: pin_for(&config),
        tokenizer: pin_for(&tokenizer),
    };
    SyntheticPackage {
        weights,
        config,
        tokenizer,
        descriptor,
    }
}
impl SyntheticPackage {
    fn bytes_for(&self, kind: ModelArtifactKind) -> &[u8] {
        match kind {
            ModelArtifactKind::Weights => &self.weights,
            ModelArtifactKind::Config => &self.config,
            ModelArtifactKind::Tokenizer => &self.tokenizer,
        }
    }
}
struct TestDir(PathBuf);
impl TestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "proteus-manager-test-{}-{}-{name}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir(&path).expect("test dir");
        Self(path)
    }
    fn cache(&self) -> PathBuf {
        self.0.join("cache")
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
/// Seeds the cache with an already verified artifact (exact pinned bytes).
fn seed_verified(cache: &Path, bytes: &[u8]) {
    fs::create_dir_all(cache).expect("cache dir");
    let pin = pin_for(bytes);
    fs::write(cache.join(&pin.sha256), bytes).expect("seed artifact");
}
/// Seeds the cache with bytes that do not match the pin.
fn seed_corrupt(cache: &Path, bytes: &[u8]) {
    fs::create_dir_all(cache).expect("cache dir");
    let pin = pin_for(bytes);
    let mut wrong = bytes.to_vec();
    wrong[0] ^= 0xFF;
    fs::write(cache.join(&pin.sha256), wrong).expect("seed corrupt artifact");
}
/// Asserts that the cache still holds exactly the verified pinned bytes.
fn assert_verified_retained(cache: &Path, bytes: &[u8], pin: &ArtifactPin) {
    let path = cache.join(&pin.sha256);
    assert_eq!(
        fs::read(&path)
            .expect("verified artifact retained")
            .as_slice(),
        bytes,
        "verified bytes must be retained for a resumable retry"
    );
}
fn cache_entries(dir: &Path) -> Vec<String> {
    let mut names = fs::read_dir(dir)
        .expect("read cache dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .into_string()
                .expect("utf-8 name")
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}
/// Shared counters proving lazy, keyed, single-reader-at-a-time source access.
struct SourceTracker<'a> {
    opens: RefCell<HashMap<ModelArtifactKind, usize>>,
    live: Cell<usize>,
    max_live: Cell<usize>,
    max_buffer: Cell<usize>,
    /// When set, the factory fails for this kind with a synthetic io error.
    fail_open_for: Cell<Option<ModelArtifactKind>>,
    /// When set, this kind's reader fails once after serving its bytes.
    fail_read_for: Cell<Option<ModelArtifactKind>>,
    /// Arm the shared cancel cell on this kind's first read / EOF.
    arm_first_read_for: Cell<Option<ModelArtifactKind>>,
    arm_eof_for: Cell<Option<ModelArtifactKind>>,
    armed: Option<&'a Cell<bool>>,
}
impl<'a> SourceTracker<'a> {
    fn new() -> Self {
        Self::new_with_arm(None)
    }
    fn new_with_arm(armed: Option<&'a Cell<bool>>) -> Self {
        Self {
            opens: RefCell::new(HashMap::new()),
            live: Cell::new(0),
            max_live: Cell::new(0),
            max_buffer: Cell::new(0),
            fail_open_for: Cell::new(None),
            fail_read_for: Cell::new(None),
            arm_first_read_for: Cell::new(None),
            arm_eof_for: Cell::new(None),
            armed,
        }
    }
    fn opens_for(&self, kind: ModelArtifactKind) -> usize {
        self.opens.borrow().get(&kind).copied().unwrap_or(0)
    }
}
struct TrackedReader<'a> {
    tracker: &'a SourceTracker<'a>,
    kind: ModelArtifactKind,
    bytes: &'a [u8],
    failed_once: bool,
}
impl Read for TrackedReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.tracker
            .max_buffer
            .set(self.tracker.max_buffer.get().max(buffer.len()));
        if self.tracker.arm_first_read_for.get() == Some(self.kind) {
            if let Some(armed) = self.tracker.armed {
                armed.set(true);
            }
            self.tracker.arm_first_read_for.set(None);
        }
        if self.bytes.is_empty() {
            if self.tracker.arm_eof_for.get() == Some(self.kind) {
                if let Some(armed) = self.tracker.armed {
                    armed.set(true);
                }
                self.tracker.arm_eof_for.set(None);
            }
            if self.tracker.fail_read_for.get() == Some(self.kind) && !self.failed_once {
                self.failed_once = true;
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "synthetic read failure",
                ));
            }
            return Ok(0);
        }
        let take = self.bytes.len().min(buffer.len());
        buffer[..take].copy_from_slice(&self.bytes[..take]);
        self.bytes = &self.bytes[take..];
        Ok(take)
    }
}
impl Drop for TrackedReader<'_> {
    fn drop(&mut self) {
        self.tracker.live.set(self.tracker.live.get() - 1);
    }
}
fn open_tracked<'a>(
    tracker: &'a SourceTracker<'a>,
    package: &'a SyntheticPackage,
    kind: ModelArtifactKind,
) -> io::Result<Box<dyn Read + 'a>> {
    *tracker.opens.borrow_mut().entry(kind).or_insert(0) += 1;
    if tracker.fail_open_for.get() == Some(kind) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "synthetic open failure",
        ));
    }
    tracker.live.set(tracker.live.get() + 1);
    tracker
        .max_live
        .set(tracker.max_live.get().max(tracker.live.get()));
    Ok(Box::new(TrackedReader {
        tracker,
        kind,
        bytes: package.bytes_for(kind),
        failed_once: false,
    }))
}
fn ready_paths(paths: &ModelPackagePaths, cache: &Path, package: &SyntheticPackage) {
    assert_eq!(
        paths.weights,
        cache.join(&package.descriptor.weights.sha256)
    );
    assert_eq!(paths.config, cache.join(&package.descriptor.config.sha256));
    assert_eq!(
        paths.tokenizer,
        cache.join(&package.descriptor.tokenizer.sha256)
    );
}
fn never_open(kind: ModelArtifactKind) -> io::Result<Box<dyn Read>> {
    panic!("verified cache must be reused without opening the source: {kind:?}")
}
#[test]
fn inspect_reports_missing_without_creating_cache_root() {
    let dir = TestDir::new("inspect-missing-root");
    let cache = dir.cache();
    let package = synthetic_package();
    let status = inspect_package_status(&cache, &package.descriptor, &|| Ok(())).expect("inspect");
    assert_eq!(status, PackageStatus::Missing);
    assert!(
        !cache.exists(),
        "inspection must never create the cache directory"
    );
}
#[test]
fn inspect_reports_missing_for_partial_package() {
    let dir = TestDir::new("inspect-partial");
    let cache = dir.cache();
    let package = synthetic_package();
    seed_verified(&cache, &package.weights);
    seed_verified(&cache, &package.config);
    let status = inspect_package_status(&cache, &package.descriptor, &|| Ok(())).expect("inspect");
    assert_eq!(status, PackageStatus::Missing, "two of three is not ready");
}
#[test]
fn inspect_reports_ready_with_native_paths_for_verified_package() {
    let dir = TestDir::new("inspect-ready");
    let cache = dir.cache();
    let package = synthetic_package();
    seed_verified(&cache, &package.weights);
    seed_verified(&cache, &package.config);
    seed_verified(&cache, &package.tokenizer);
    let status = inspect_package_status(&cache, &package.descriptor, &|| Ok(())).expect("inspect");
    let PackageStatus::Ready { paths } = status else {
        panic!("expected ready, got {status:?}");
    };
    ready_paths(&paths, &cache, &package);
    for (path, bytes) in [
        (&paths.weights, &package.weights),
        (&paths.config, &package.config),
        (&paths.tokenizer, &package.tokenizer),
    ] {
        assert_eq!(
            fs::read(path).expect("artifact").as_slice(),
            bytes.as_slice()
        );
    }
}
#[test]
fn inspect_rejects_corrupt_existing_artifact_and_leaves_it_untouched() {
    let dir = TestDir::new("inspect-corrupt");
    let cache = dir.cache();
    let package = synthetic_package();
    seed_corrupt(&cache, &package.weights);
    let error = inspect_package_status(&cache, &package.descriptor, &|| Ok(()))
        .expect_err("corrupt artifact is an explicit error");
    assert!(matches!(error, CacheInstallError::ExistingArtifactCorrupt));
    let corrupt = fs::read(cache.join(&package.descriptor.weights.sha256)).expect("corrupt file");
    assert_ne!(corrupt, package.weights, "corrupt bytes must be preserved");
}
#[test]
fn inspect_refuses_directory_at_an_artifact_path() {
    let dir = TestDir::new("inspect-blocked");
    let cache = dir.cache();
    fs::create_dir_all(&cache).expect("cache dir");
    let package = synthetic_package();
    let path = cache.join(&package.descriptor.weights.sha256);
    fs::create_dir(&path).expect("directory at artifact path");
    let error = inspect_package_status(&cache, &package.descriptor, &|| Ok(()))
        .expect_err("blocked path is an explicit error");
    assert!(matches!(error, CacheInstallError::ArtifactPathBlocked));
    assert!(path.is_dir());
}
#[cfg(unix)]
#[test]
fn inspect_and_install_refuse_symlinked_cache_directory() {
    use std::os::unix::fs::symlink;
    let dir = TestDir::new("symlinked-cache");
    let real = dir.0.join("real-cache");
    fs::create_dir(&real).expect("real cache dir");
    let link = dir.0.join("linked-cache");
    symlink(&real, &link).expect("cache symlink");
    let package = synthetic_package();
    let error = inspect_package_status(&link, &package.descriptor, &|| Ok(()))
        .expect_err("refused cache dir");
    assert!(matches!(error, CacheInstallError::CacheDirectoryInvalid));
    let tracker = SourceTracker::new();
    let error = install_package(
        &link,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &|| Ok(()),
        None,
    )
    .expect_err("refused cache dir");
    assert!(matches!(error, CacheInstallError::CacheDirectoryInvalid));
    assert_eq!(cache_entries(&real), Vec::<String>::new());
}
#[test]
fn install_fetches_all_three_artifacts_and_reports_ready() {
    let dir = TestDir::new("install-all");
    let cache = dir.cache();
    let package = synthetic_package();
    let tracker = SourceTracker::new();
    let mut progress = Vec::new();
    let paths = install_package(
        &cache,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &|| Ok(()),
        Some(&mut |report| progress.push(report)),
    )
    .expect("install");
    ready_paths(&paths, &cache, &package);
    for kind in MODEL_ARTIFACT_KINDS {
        assert_eq!(
            tracker.opens_for(kind),
            1,
            "one open per artifact: {kind:?}"
        );
    }
    assert_eq!(fs::read(&paths.weights).expect("weights"), package.weights);
    let total = package.descriptor.weights.bytes
        + package.descriptor.config.bytes
        + package.descriptor.tokenizer.bytes;
    assert_eq!(
        progress.last().copied(),
        Some(InstallProgress {
            artifacts_verified: 3,
            bytes_verified: total,
        })
    );
    let status = inspect_package_status(&cache, &package.descriptor, &|| Ok(())).expect("inspect");
    assert!(matches!(status, PackageStatus::Ready { .. }));
}
#[test]
fn install_cache_hit_never_opens_the_source() {
    let dir = TestDir::new("install-cache-hit");
    let cache = dir.cache();
    let package = synthetic_package();
    seed_verified(&cache, &package.weights);
    seed_verified(&cache, &package.config);
    seed_verified(&cache, &package.tokenizer);
    let tracker = SourceTracker::new();
    let paths = install_package(
        &cache,
        &package.descriptor,
        &mut never_open,
        &|| Ok(()),
        None,
    )
    .expect("install");
    ready_paths(&paths, &cache, &package);
    assert_eq!(tracker.max_live.get(), 0);
}
#[test]
fn install_opens_the_source_only_for_missing_artifacts() {
    let dir = TestDir::new("install-partial-hit");
    let cache = dir.cache();
    let package = synthetic_package();
    seed_verified(&cache, &package.weights);
    let tracker = SourceTracker::new();
    let paths = install_package(
        &cache,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &|| Ok(()),
        None,
    )
    .expect("install");
    ready_paths(&paths, &cache, &package);
    assert_eq!(tracker.opens_for(ModelArtifactKind::Weights), 0);
    assert_eq!(tracker.opens_for(ModelArtifactKind::Config), 1);
    assert_eq!(tracker.opens_for(ModelArtifactKind::Tokenizer), 1);
}
#[test]
fn install_keeps_at_most_one_live_reader_and_bounded_chunks() {
    let dir = TestDir::new("install-one-reader");
    let cache = dir.cache();
    let mut package = synthetic_package();
    // Force multi-chunk streaming through the bounded adapter.
    package.weights = vec![0x5Au8; INSTALL_CHUNK_BYTES + 3];
    package.descriptor.weights = pin_for(&package.weights);
    let tracker = SourceTracker::new();
    install_package(
        &cache,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &|| Ok(()),
        None,
    )
    .expect("install");
    assert_eq!(
        tracker.max_live.get(),
        1,
        "at most one source reader may be live at a time"
    );
    assert!(
        tracker.max_buffer.get() <= INSTALL_CHUNK_BYTES,
        "single read buffer {} exceeded the chunk bound",
        tracker.max_buffer.get()
    );
    assert!(
        tracker.max_buffer.get() >= 2,
        "fixture must actually observe read buffer sizes"
    );
}
#[test]
fn install_rejects_wrong_digest_stream_and_reports_missing() {
    let dir = TestDir::new("install-wrong-digest");
    let cache = dir.cache();
    let package = synthetic_package();
    let mut corrupted = package.weights.clone();
    corrupted[0] ^= 0xFF;
    let mut corrupted_factory = move |kind| {
        if kind == ModelArtifactKind::Weights {
            Ok(Box::new(Cursor::new(corrupted.clone())) as Box<dyn Read>)
        } else {
            panic!("weights digest failure must abort before other artifacts")
        }
    };
    let error = install_package(
        &cache,
        &package.descriptor,
        &mut corrupted_factory,
        &|| Ok(()),
        None,
    )
    .expect_err("wrong digest must fail");
    assert!(matches!(error, CacheInstallError::ArtifactChecksumMismatch));
    let status = inspect_package_status(&cache, &package.descriptor, &|| Ok(())).expect("inspect");
    assert_eq!(status, PackageStatus::Missing);
}
#[test]
fn install_rejects_truncated_stream() {
    let dir = TestDir::new("install-truncated");
    let cache = dir.cache();
    let package = synthetic_package();
    let truncated = package.config[..package.config.len() - 1].to_vec();
    let weights = package.weights.clone();
    let descriptor = package.descriptor.clone();
    let mut factory = move |kind| -> io::Result<Box<dyn Read>> {
        match kind {
            ModelArtifactKind::Weights => Ok(Box::new(Cursor::new(weights.clone()))),
            ModelArtifactKind::Config => Ok(Box::new(Cursor::new(truncated.clone()))),
            ModelArtifactKind::Tokenizer => panic!("truncated config must abort the install"),
        }
    };
    let error = install_package(&cache, &descriptor, &mut factory, &|| Ok(()), None)
        .expect_err("truncated stream must fail");
    assert!(matches!(error, CacheInstallError::ArtifactTruncated));
    assert_eq!(
        cache_entries(&cache),
        vec![descriptor.weights.sha256.clone()],
        "the earlier verified artifact stays, the failed one is absent"
    );
}
#[test]
fn install_rejects_oversized_stream() {
    let dir = TestDir::new("install-oversized");
    let cache = dir.cache();
    let package = synthetic_package();
    let mut oversized = package.tokenizer.clone();
    oversized.push(0x00);
    let weights = package.weights.clone();
    let config = package.config.clone();
    let descriptor = package.descriptor.clone();
    let mut factory = move |kind| -> io::Result<Box<dyn Read>> {
        match kind {
            ModelArtifactKind::Weights => Ok(Box::new(Cursor::new(weights.clone()))),
            ModelArtifactKind::Config => Ok(Box::new(Cursor::new(config.clone()))),
            ModelArtifactKind::Tokenizer => Ok(Box::new(Cursor::new(oversized.clone()))),
        }
    };
    let error = install_package(&cache, &descriptor, &mut factory, &|| Ok(()), None)
        .expect_err("oversized stream must fail");
    assert!(matches!(error, CacheInstallError::ArtifactOversized));
    assert_eq!(
        cache_entries(&cache).len(),
        2,
        "weights and config stay verified"
    );
}
#[test]
fn factory_failure_on_second_artifact_keeps_first_verified_and_retry_reuses_it() {
    let dir = TestDir::new("install-factory-failure");
    let cache = dir.cache();
    let package = synthetic_package();
    let tracker = SourceTracker::new();
    tracker.fail_open_for.set(Some(ModelArtifactKind::Config));
    let error = install_package(
        &cache,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &|| Ok(()),
        None,
    )
    .expect_err("factory failure must surface");
    assert!(matches!(error, CacheInstallError::SourceFailed(_)));
    assert_verified_retained(&cache, &package.weights, &package.descriptor.weights);
    // Retry with a working factory: weights were verified and must be reused.
    tracker.fail_open_for.set(None);
    let paths = install_package(
        &cache,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &|| Ok(()),
        None,
    )
    .expect("retry install");
    ready_paths(&paths, &cache, &package);
    assert_eq!(
        tracker.opens_for(ModelArtifactKind::Weights),
        1,
        "verified weights must not be re-opened on retry"
    );
}
#[test]
fn read_failure_on_third_artifact_keeps_earlier_verified_and_retry_reuses() {
    let dir = TestDir::new("install-read-failure");
    let cache = dir.cache();
    let package = synthetic_package();
    let tracker = SourceTracker::new();
    tracker
        .fail_read_for
        .set(Some(ModelArtifactKind::Tokenizer));
    let error = install_package(
        &cache,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &|| Ok(()),
        None,
    )
    .expect_err("read failure must surface");
    assert!(matches!(error, CacheInstallError::SourceFailed(_)));
    assert_verified_retained(&cache, &package.weights, &package.descriptor.weights);
    assert_verified_retained(&cache, &package.config, &package.descriptor.config);
    tracker.fail_read_for.set(None);
    let paths = install_package(
        &cache,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &|| Ok(()),
        None,
    )
    .expect("retry install");
    ready_paths(&paths, &cache, &package);
    assert_eq!(
        tracker.opens_for(ModelArtifactKind::Weights),
        1,
        "verified weights must not be re-opened on retry"
    );
    assert_eq!(
        tracker.opens_for(ModelArtifactKind::Config),
        1,
        "verified config must not be re-opened on retry"
    );
    assert_eq!(tracker.opens_for(ModelArtifactKind::Tokenizer), 2);
}
#[test]
fn cancellation_during_final_artifact_keeps_earlier_verified() {
    let dir = TestDir::new("cancel-during-final");
    let cache = dir.cache();
    let package = synthetic_package();
    let armed = Cell::new(false);
    let tracker = SourceTracker::new_with_arm(Some(&armed));
    tracker
        .arm_first_read_for
        .set(Some(ModelArtifactKind::Tokenizer));
    let cancel_when_armed = || {
        if armed.get() {
            Err("cancelled by test".to_string())
        } else {
            Ok(())
        }
    };
    let error = install_package(
        &cache,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &cancel_when_armed,
        None,
    )
    .expect_err("cancellation must surface");
    assert!(matches!(error, CacheInstallError::Cancelled(_)));
    assert_verified_retained(&cache, &package.weights, &package.descriptor.weights);
    assert_verified_retained(&cache, &package.config, &package.descriptor.config);
    // Retry succeeds without re-opening the retained verified artifacts.
    let paths = install_package(
        &cache,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &|| Ok(()),
        None,
    )
    .expect("retry install");
    ready_paths(&paths, &cache, &package);
    assert_eq!(
        tracker.opens_for(ModelArtifactKind::Weights),
        1,
        "verified weights must not be re-opened on retry"
    );
    assert_eq!(
        tracker.opens_for(ModelArtifactKind::Config),
        1,
        "verified config must not be re-opened on retry"
    );
    assert_eq!(tracker.opens_for(ModelArtifactKind::Tokenizer), 2);
}
#[test]
fn cancellation_after_verification_of_final_artifact_never_reports_ready() {
    let dir = TestDir::new("cancel-after-eof");
    let cache = dir.cache();
    let package = synthetic_package();
    let armed = Cell::new(false);
    let tracker = SourceTracker::new_with_arm(Some(&armed));
    tracker.arm_eof_for.set(Some(ModelArtifactKind::Tokenizer));
    let cancel_when_armed = || {
        if armed.get() {
            Err("cancelled by test".to_string())
        } else {
            Ok(())
        }
    };
    let error = install_package(
        &cache,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &cancel_when_armed,
        None,
    )
    .expect_err("cancellation at EOF must surface");
    assert!(matches!(error, CacheInstallError::Cancelled(_)));
    assert_eq!(
        cache_entries(&cache),
        vec![
            package.descriptor.weights.sha256.clone(),
            package.descriptor.config.sha256.clone(),
        ],
        "the discarded temporary must not occupy the artifact path"
    );
    let status = inspect_package_status(&cache, &package.descriptor, &|| Ok(())).expect("inspect");
    assert_eq!(
        status,
        PackageStatus::Missing,
        "cancelled package is not ready"
    );
}
#[test]
fn cancellation_before_any_io_creates_nothing() {
    let dir = TestDir::new("cancel-before-io");
    let cache = dir.cache();
    let package = synthetic_package();
    let tracker = SourceTracker::new();
    let error = install_package(
        &cache,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &|| Err("cancelled before io".to_string()),
        None,
    )
    .expect_err("pre-io cancellation must surface");
    assert!(matches!(error, CacheInstallError::Cancelled(_)));
    assert!(!cache.exists(), "no cache IO may have happened");
    assert_eq!(tracker.max_live.get(), 0);
}
#[test]
fn repeated_install_and_simulated_restart_are_idempotent() {
    let dir = TestDir::new("install-idempotent");
    let cache = dir.cache();
    let package = synthetic_package();
    let tracker = SourceTracker::new();
    let first = install_package(
        &cache,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &|| Ok(()),
        None,
    )
    .expect("first install");
    // A "restart" is a fresh call against the persisted cache: no re-opens.
    let second = install_package(
        &cache,
        &package.descriptor,
        &mut never_open,
        &|| Ok(()),
        None,
    )
    .expect("second install");
    assert_eq!(first, second);
    for kind in MODEL_ARTIFACT_KINDS {
        assert_eq!(tracker.opens_for(kind), 1);
    }
}
#[test]
fn progress_reports_only_verified_facts() {
    let dir = TestDir::new("install-progress");
    let cache = dir.cache();
    let package = synthetic_package();
    seed_verified(&cache, &package.weights);
    let tracker = SourceTracker::new();
    let progress = RefCell::new(Vec::new());
    install_package(
        &cache,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &|| Ok(()),
        Some(&mut |report| progress.borrow_mut().push(report)),
    )
    .expect("install");
    let progress = progress.into_inner();
    assert_eq!(progress.len(), 3, "one report per verified artifact");
    let total = package.descriptor.weights.bytes
        + package.descriptor.config.bytes
        + package.descriptor.tokenizer.bytes;
    for (index, report) in progress.iter().enumerate() {
        assert_eq!(report.artifacts_verified, index as u32 + 1);
        assert!(report.bytes_verified <= total);
    }
    assert_eq!(
        progress.last().copied(),
        Some(InstallProgress {
            artifacts_verified: 3,
            bytes_verified: total,
        })
    );
}

#[test]
fn final_progress_cancellation_never_returns_a_ready_package() {
    let dir = TestDir::new("cancel-final-progress");
    let cache = dir.cache();
    let package = synthetic_package();
    let tracker = SourceTracker::new();
    let cancelled = Cell::new(false);
    let result = install_package(
        &cache,
        &package.descriptor,
        &mut |kind| open_tracked(&tracker, &package, kind),
        &|| {
            if cancelled.get() {
                Err("cancel final progress  ".into())
            } else {
                Ok(())
            }
        },
        Some(&mut |progress| {
            if progress.artifacts_verified == 3 {
                cancelled.set(true);
            }
        }),
    );
    assert!(
        matches!(result, Err(CacheInstallError::Cancelled(ref message)) if message == "cancel final progress  ")
    );
    assert!(matches!(
        inspect_package_status(&cache, &package.descriptor, &|| Ok(())).unwrap(),
        PackageStatus::Ready { .. }
    ));
}
#[test]
fn inspection_reports_corrupt_existing_config_even_when_weights_are_missing() {
    let dir = TestDir::new("partial-corrupt");
    let cache = dir.cache();
    let package = synthetic_package();
    seed_corrupt(&cache, &package.config);
    let before = fs::read(cache.join(&package.descriptor.config.sha256)).unwrap();
    let result = inspect_package_status(&cache, &package.descriptor, &|| Ok(()));
    assert!(matches!(
        result,
        Err(CacheInstallError::ExistingArtifactCorrupt)
    ));
    assert_eq!(
        fs::read(cache.join(&package.descriptor.config.sha256)).unwrap(),
        before
    );
}
