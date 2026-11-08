use super::*;
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn pin_for(bytes: &[u8]) -> ArtifactPin {
    ArtifactPin::new(bytes.len() as u64, hex_digest(bytes)).expect("valid pin")
}
struct TestDir(PathBuf);
impl TestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "proteus-cache-test-{}-{}-{name}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir(&path).expect("test dir");
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
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
struct ChunkedReader<'a> {
    bytes: &'a [u8],
    step: usize,
    reads: Cell<usize>,
}
impl<'a> ChunkedReader<'a> {
    fn new(bytes: &'a [u8], step: usize) -> Self {
        Self {
            bytes,
            step,
            reads: Cell::new(0),
        }
    }
}
impl Read for ChunkedReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.reads.set(self.reads.get() + 1);
        if self.bytes.is_empty() {
            return Ok(0);
        }
        let take = self.step.min(self.bytes.len()).min(buffer.len());
        buffer[..take].copy_from_slice(&self.bytes[..take]);
        self.bytes = &self.bytes[take..];
        Ok(take)
    }
}
struct EndlessReader {
    reads: Cell<usize>,
}
impl Read for EndlessReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.reads.set(self.reads.get() + 1);
        buffer.fill(0xAB);
        Ok(buffer.len())
    }
}
struct FailingReader<'a> {
    bytes: &'a [u8],
}
impl Read for FailingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.bytes.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "synthetic read failure",
            ));
        }
        let take = self.bytes.len().min(buffer.len());
        buffer[..take].copy_from_slice(&self.bytes[..take]);
        self.bytes = &self.bytes[take..];
        Ok(take)
    }
}
struct UntouchedReader;
impl Read for UntouchedReader {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        panic!("verified cache must be reused without reading the source");
    }
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
fn counted_check(calls: &Cell<usize>, fail_at: usize) -> impl Fn() -> Result<(), String> + '_ {
    move || {
        calls.set(calls.get() + 1);
        if calls.get() == fail_at {
            Err("cancelled by test".into())
        } else {
            Ok(())
        }
    }
}
#[test]
fn pin_rejects_zero_bytes_and_malformed_digest() {
    let digest = "a1".repeat(32);
    let malformed = vec![
        String::new(),
        "a".repeat(63),
        "A".repeat(64),
        "g".repeat(64),
        "a1".repeat(31) + "zz",
    ];
    for sha256 in &malformed {
        assert!(
            matches!(
                ArtifactPin::new(10, sha256.as_str()),
                Err(CacheInstallError::InvalidPin)
            ),
            "digest: {sha256}"
        );
    }
    assert!(matches!(
        ArtifactPin::new(0, digest.clone()),
        Err(CacheInstallError::InvalidPin)
    ));
    assert!(ArtifactPin::new(10, digest).is_ok());
}
#[test]
fn installs_streamed_bytes_and_returns_verified_content_addressed_path() {
    let dir = TestDir::new("install");
    let cache = dir.cache();
    let payload: Vec<u8> = (0u8..=255).cycle().take(11).collect();
    let pin = pin_for(&payload);
    let mut reader = ChunkedReader::new(&payload, 3);
    let path = install_verified_artifact(&cache, &pin, &mut reader, &|| Ok(())).expect("install");
    assert_eq!(path, cache.join(hex_digest(&payload)));
    assert_eq!(fs::read(&path).expect("artifact"), payload);
    assert_eq!(fs::metadata(&path).expect("metadata").len(), pin.bytes);
    assert_eq!(
        reader.reads.get(),
        5,
        "3+3+3+2 payload reads plus one EOF read"
    );
    assert_eq!(cache_entries(&cache), vec![hex_digest(&payload)]);
}
#[test]
fn verified_cache_is_reused_without_reading_the_source() {
    let dir = TestDir::new("reuse");
    let cache = dir.cache();
    fs::create_dir(&cache).expect("cache dir");
    let payload = b"cached model bytes";
    let pin = pin_for(payload);
    let path = cache.join(&pin.sha256);
    fs::write(&path, payload).expect("seed cache");
    let mut reader = UntouchedReader;
    let installed =
        install_verified_artifact(&cache, &pin, &mut reader, &|| Ok(())).expect("reuse");
    assert_eq!(installed, path);
    assert_eq!(fs::read(&path).expect("artifact"), payload);
}
#[test]
fn verified_reuse_applies_the_deadline_check_at_eof() {
    // Check-call order when an already verified artifact occupies the path:
    // 1 entry, 2 before the first read, 3 after IO, 4 before the EOF read,
    // 5 the final recheck after EOF. Without that last recheck a deadline
    // expiring exactly at EOF is silently ignored on the verified-reuse path.
    let dir = TestDir::new("reuse-eof-cancel");
    let cache = dir.cache();
    fs::create_dir(&cache).expect("cache dir");
    let payload = b"cached model bytes";
    let pin = pin_for(payload);
    let path = cache.join(&pin.sha256);
    fs::write(&path, payload).expect("seed cache");
    let calls = Cell::new(0);
    let mut reader = UntouchedReader;
    let error = install_verified_artifact(&cache, &pin, &mut reader, &counted_check(&calls, 5))
        .expect_err("cancelled at EOF during verified reuse");
    assert!(matches!(error, CacheInstallError::Cancelled(_)));
    assert_eq!(calls.get(), 5, "the final recheck must run exactly once");
    assert_eq!(fs::read(&path).expect("artifact"), payload);
    assert_eq!(cache_entries(&cache), vec![pin.sha256.clone()]);
}
#[test]
fn truncated_stream_is_rejected_and_leaves_no_artifact() {
    let dir = TestDir::new("truncated");
    let cache = dir.cache();
    let full = b"exact pinned artifact bytes";
    let pin = pin_for(full);
    let mut reader = ChunkedReader::new(&full[..full.len() - 1], 4);
    let error =
        install_verified_artifact(&cache, &pin, &mut reader, &|| Ok(())).expect_err("truncated");
    assert!(matches!(error, CacheInstallError::ArtifactTruncated));
    assert_eq!(cache_entries(&cache), Vec::<String>::new());
}
#[test]
fn oversized_stream_is_rejected_after_the_first_bounded_read() {
    let dir = TestDir::new("oversized");
    let cache = dir.cache();
    let pin = pin_for(b"tiny");
    let mut reader = EndlessReader {
        reads: Cell::new(0),
    };
    let error =
        install_verified_artifact(&cache, &pin, &mut reader, &|| Ok(())).expect_err("oversized");
    assert!(matches!(error, CacheInstallError::ArtifactOversized));
    assert_eq!(
        reader.reads.get(),
        1,
        "bounded rejection must not drain the source"
    );
    assert_eq!(cache_entries(&cache), Vec::<String>::new());
}
#[test]
fn same_length_wrong_bytes_fail_the_final_digest() {
    let dir = TestDir::new("digest");
    let cache = dir.cache();
    let payload = b"digest me exactly";
    let mut other = payload.to_vec();
    other[0] ^= 0xFF;
    let pin = pin_for(payload);
    let mut reader = ChunkedReader::new(&other, 4);
    let error =
        install_verified_artifact(&cache, &pin, &mut reader, &|| Ok(())).expect_err("digest");
    assert!(matches!(error, CacheInstallError::ArtifactChecksumMismatch));
    assert_eq!(cache_entries(&cache), Vec::<String>::new());
}
#[test]
fn source_read_failure_is_reported_and_cleans_the_temporary() {
    let dir = TestDir::new("read-failure");
    let cache = dir.cache();
    let payload = b"partial then failure";
    let pin = pin_for(payload);
    let mut reader = FailingReader {
        bytes: &payload[..6],
    };
    let error =
        install_verified_artifact(&cache, &pin, &mut reader, &|| Ok(())).expect_err("read failure");
    assert!(matches!(error, CacheInstallError::SourceFailed(_)));
    assert_eq!(cache_entries(&cache), Vec::<String>::new());
}
#[test]
fn cancellation_after_the_final_read_discards_the_temporary() {
    // Check-call order for a fresh single-read install: 1 entry, 2 before the
    // first read, 3 after IO, 4 before the EOF read, 5 after EOF, 6 pre-publish.
    let dir = TestDir::new("cancel-read");
    let cache = dir.cache();
    let payload = b"final";
    let pin = pin_for(payload);
    let calls = Cell::new(0);
    let mut reader = ChunkedReader::new(payload, 5);
    let error = install_verified_artifact(&cache, &pin, &mut reader, &counted_check(&calls, 3))
        .expect_err("cancelled");
    assert!(matches!(error, CacheInstallError::Cancelled(_)));
    assert_eq!(cache_entries(&cache), Vec::<String>::new());
}
#[test]
fn cancellation_before_publish_discards_the_verified_temporary() {
    let dir = TestDir::new("cancel-publish");
    let cache = dir.cache();
    let payload = b"final";
    let pin = pin_for(payload);
    let calls = Cell::new(0);
    let mut reader = ChunkedReader::new(payload, 5);
    let error = install_verified_artifact(&cache, &pin, &mut reader, &counted_check(&calls, 6))
        .expect_err("cancelled");
    assert!(matches!(error, CacheInstallError::Cancelled(_)));
    assert_eq!(cache_entries(&cache), Vec::<String>::new());
}
#[test]
fn reinstall_with_mismatched_source_preserves_the_verified_cache() {
    // A verified artifact already occupies the content-addressed path, so the
    // pin fully determines its content: a failing or mismatched re-install
    // must reuse the cache without consuming the source, never destroying it.
    let dir = TestDir::new("preserve-good");
    let cache = dir.cache();
    fs::create_dir(&cache).expect("cache dir");
    let payload = b"verified artifact";
    let pin = pin_for(payload);
    let path = cache.join(&pin.sha256);
    fs::write(&path, payload).expect("seed cache");
    let mut other = payload.to_vec();
    other[0] ^= 0xFF;
    let mut reader = ChunkedReader::new(&other, 4);
    let installed =
        install_verified_artifact(&cache, &pin, &mut reader, &|| Ok(())).expect("reuse");
    assert_eq!(installed, path);
    assert_eq!(
        reader.reads.get(),
        0,
        "verified cache must not be re-streamed"
    );
    assert_eq!(fs::read(&path).expect("artifact"), payload);
    assert_eq!(cache_entries(&cache), vec![pin.sha256.clone()]);
}
#[test]
fn corrupt_existing_artifact_is_reported_and_left_untouched() {
    let dir = TestDir::new("corrupt-existing");
    let cache = dir.cache();
    fs::create_dir(&cache).expect("cache dir");
    let payload = b"pinned artifact";
    let pin = pin_for(payload);
    let path = cache.join(&pin.sha256);
    let mut wrong = payload.to_vec();
    wrong[3] ^= 0xFF;
    fs::write(&path, &wrong).expect("seed corrupt cache");
    let mut reader = UntouchedReader;
    let error = install_verified_artifact(&cache, &pin, &mut reader, &|| Ok(()))
        .expect_err("corrupt cache");
    assert!(matches!(error, CacheInstallError::ExistingArtifactCorrupt));
    assert_eq!(fs::read(&path).expect("corrupt file"), wrong);
}
#[test]
fn directory_at_the_artifact_path_is_refused_before_reading() {
    let dir = TestDir::new("directory-blocked");
    let cache = dir.cache();
    fs::create_dir_all(&cache).expect("cache dir");
    let pin = pin_for(b"blocked");
    let path = cache.join(&pin.sha256);
    fs::create_dir(&path).expect("directory at artifact path");
    let mut reader = UntouchedReader;
    let error =
        install_verified_artifact(&cache, &pin, &mut reader, &|| Ok(())).expect_err("blocked path");
    assert!(matches!(error, CacheInstallError::ArtifactPathBlocked));
    assert!(path.is_dir());
}
#[cfg(unix)]
#[test]
fn symlink_at_the_artifact_path_is_refused() {
    use std::os::unix::fs::symlink;
    let dir = TestDir::new("symlink-blocked");
    let cache = dir.cache();
    fs::create_dir_all(&cache).expect("cache dir");
    let payload = b"real bytes elsewhere";
    let source = dir.path().join("source");
    fs::write(&source, payload).expect("source file");
    let pin = pin_for(payload);
    let path = cache.join(&pin.sha256);
    symlink(&source, &path).expect("symlink");
    let mut reader = UntouchedReader;
    let error =
        install_verified_artifact(&cache, &pin, &mut reader, &|| Ok(())).expect_err("blocked path");
    assert!(matches!(error, CacheInstallError::ArtifactPathBlocked));
    assert!(fs::symlink_metadata(&path)
        .expect("metadata")
        .file_type()
        .is_symlink());
}
#[cfg(unix)]
#[test]
fn socket_at_the_artifact_path_is_refused() {
    // Acceptance must require a regular file, not merely "not a directory and
    // not a symlink": a socket (or fifo/device) must be refused before any
    // content is read. Pure-std fixture, no external commands. macOS limits
    // unix socket paths to SUN_LEN, so this cache path stays short.
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::net::UnixListener;
    let short_root = Path::new("/tmp").join(format!("pc{:016x}", rand::random::<u64>()));
    fs::create_dir(&short_root).expect("short test root");
    let dir = TestDir(short_root);
    let cache = dir.path().join("c");
    fs::create_dir(&cache).expect("cache dir");
    let pin = pin_for(b"blocked");
    let path = cache.join(&pin.sha256);
    let listener = UnixListener::bind(&path).expect("socket at artifact path");
    let mut reader = UntouchedReader;
    let error =
        install_verified_artifact(&cache, &pin, &mut reader, &|| Ok(())).expect_err("blocked path");
    assert!(matches!(error, CacheInstallError::ArtifactPathBlocked));
    drop(listener);
    assert!(fs::symlink_metadata(&path)
        .expect("metadata")
        .file_type()
        .is_socket());
}
#[cfg(unix)]
#[test]
fn symlinked_cache_directory_is_refused() {
    use std::os::unix::fs::symlink;
    let dir = TestDir::new("symlinked-cache");
    let real = dir.path().join("real-cache");
    fs::create_dir(&real).expect("real cache dir");
    let link = dir.path().join("linked-cache");
    symlink(&real, &link).expect("cache symlink");
    let pin = pin_for(b"anything");
    let mut reader = UntouchedReader;
    let error = install_verified_artifact(&link, &pin, &mut reader, &|| Ok(()))
        .expect_err("refused cache dir");
    assert!(matches!(error, CacheInstallError::CacheDirectoryInvalid));
    assert_eq!(cache_entries(&real), Vec::<String>::new());
}
#[test]
fn plain_file_as_cache_directory_is_rejected() {
    let dir = TestDir::new("file-cache");
    let file_path = dir.path().join("not-a-directory");
    fs::write(&file_path, b"x").expect("file at cache path");
    let pin = pin_for(b"anything");
    let mut reader = UntouchedReader;
    let error = install_verified_artifact(&file_path, &pin, &mut reader, &|| Ok(()))
        .expect_err("rejected cache dir");
    assert!(matches!(error, CacheInstallError::CacheDirectoryInvalid));
}
#[test]
fn concurrent_same_pin_installs_complete_once_and_all_reuse_the_verified_path() {
    let dir = TestDir::new("concurrent");
    let cache = dir.cache();
    fs::create_dir(&cache).expect("cache dir");
    let payload: Vec<u8> = (0..1000u32).map(|index| (index % 251) as u8).collect();
    let pin = pin_for(&payload);
    std::thread::scope(|scope| {
        let payload = &payload;
        let cache = &cache;
        let pin = &pin;
        let mut handles = Vec::new();
        for step in [1usize, 7, 64, 1000] {
            handles.push(scope.spawn(move || {
                let mut reader = ChunkedReader::new(payload, step);
                install_verified_artifact(cache, pin, &mut reader, &|| Ok(()))
            }));
        }
        for handle in handles {
            let installed = handle.join().expect("install thread").expect("install");
            assert_eq!(installed, cache.join(&pin.sha256));
        }
    });
    assert_eq!(
        fs::read(cache.join(&pin.sha256)).expect("artifact"),
        payload
    );
    assert_eq!(cache_entries(&cache), vec![pin.sha256.clone()]);
}
#[test]
fn lookup_reports_none_for_missing_cache_root_without_creating_it() {
    let dir = TestDir::new("lookup-missing-root");
    let cache = dir.cache();
    let pin = pin_for(b"anything");
    let found = lookup_verified(&cache, &pin, &|| Ok(())).expect("lookup");
    assert_eq!(found, None, "missing cache root must report None");
    assert!(
        !cache.exists(),
        "inspection must never create the cache directory"
    );
}
#[test]
fn lookup_reports_none_for_missing_artifact() {
    let dir = TestDir::new("lookup-missing-artifact");
    let cache = dir.cache();
    fs::create_dir(&cache).expect("cache dir");
    let pin = pin_for(b"absent");
    let found = lookup_verified(&cache, &pin, &|| Ok(())).expect("lookup");
    assert_eq!(found, None);
    assert_eq!(cache_entries(&cache), Vec::<String>::new());
}
#[test]
fn lookup_returns_the_verified_path_and_uses_the_deadline_check_at_eof() {
    let dir = TestDir::new("lookup-verified");
    let cache = dir.cache();
    fs::create_dir(&cache).expect("cache dir");
    let payload = b"verified lookup bytes";
    let pin = pin_for(payload);
    let path = cache.join(&pin.sha256);
    fs::write(&path, payload).expect("seed cache");
    let found = lookup_verified(&cache, &pin, &|| Ok(())).expect("lookup");
    assert_eq!(found, Some(path.clone()));
    // Verification check cadence mirrors install: entry, before first read,
    // after IO, before the EOF read, after EOF, final recheck. A deadline
    // expiring at the EOF boundary must cancel instead of reporting ready.
    let calls = Cell::new(0);
    let error = lookup_verified(&cache, &pin, &counted_check(&calls, 5)).expect_err("cancelled");
    assert!(matches!(error, CacheInstallError::Cancelled(_)));
    assert_eq!(calls.get(), 5);
    assert_eq!(fs::read(&path).expect("artifact"), payload);
}
#[test]
fn lookup_reports_corrupt_existing_artifact_and_leaves_it_untouched() {
    let dir = TestDir::new("lookup-corrupt");
    let cache = dir.cache();
    fs::create_dir(&cache).expect("cache dir");
    let payload = b"pinned lookup artifact";
    let pin = pin_for(payload);
    let path = cache.join(&pin.sha256);
    let mut wrong = payload.to_vec();
    wrong[0] ^= 0xFF;
    fs::write(&path, &wrong).expect("seed corrupt cache");
    let error = lookup_verified(&cache, &pin, &|| Ok(())).expect_err("corrupt");
    assert!(matches!(error, CacheInstallError::ExistingArtifactCorrupt));
    assert_eq!(fs::read(&path).expect("corrupt file"), wrong);
}
#[test]
fn lookup_refuses_a_directory_at_the_artifact_path() {
    let dir = TestDir::new("lookup-directory-blocked");
    let cache = dir.cache();
    fs::create_dir_all(&cache).expect("cache dir");
    let pin = pin_for(b"blocked");
    let path = cache.join(&pin.sha256);
    fs::create_dir(&path).expect("directory at artifact path");
    let error = lookup_verified(&cache, &pin, &|| Ok(())).expect_err("blocked path");
    assert!(matches!(error, CacheInstallError::ArtifactPathBlocked));
    assert!(path.is_dir());
}
#[cfg(unix)]
#[test]
fn lookup_refuses_a_symlinked_cache_directory() {
    use std::os::unix::fs::symlink;
    let dir = TestDir::new("lookup-symlinked-cache");
    let real = dir.path().join("real-cache");
    fs::create_dir(&real).expect("real cache dir");
    let link = dir.path().join("linked-cache");
    symlink(&real, &link).expect("cache symlink");
    let pin = pin_for(b"anything");
    let error = lookup_verified(&link, &pin, &|| Ok(())).expect_err("refused cache dir");
    assert!(matches!(error, CacheInstallError::CacheDirectoryInvalid));
    assert_eq!(cache_entries(&real), Vec::<String>::new());
}
#[test]
fn lookup_rejects_a_structurally_invalid_pin() {
    let dir = TestDir::new("lookup-invalid-pin");
    let cache = dir.cache();
    fs::create_dir(&cache).expect("cache dir");
    let pin = ArtifactPin {
        bytes: 0,
        sha256: "a1".repeat(32),
    };
    let error = lookup_verified(&cache, &pin, &|| Ok(())).expect_err("invalid pin");
    assert!(matches!(error, CacheInstallError::InvalidPin));
}
