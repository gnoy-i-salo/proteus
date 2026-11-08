//! Synthetic loopback tests for the bounded download transport. Every test
//! uses a tiny std-socket HTTP server on 127.0.0.1; no real external network
//! is touched and no real model bytes exist here.
use super::*;
use crate::local_embedding::cache::{ArtifactPin, CacheInstallError};
use crate::local_embedding::manager::{InstallProgress, ModelArtifactKind, ModelPackageDescriptor};
use crate::local_embedding::runtime::DownloadDescriptor;
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const TEST_MODEL_ID: &str = "intfloat/multilingual-e5-small";
const TEST_REVISION: &str = "614241f622f53c4eeff9890bdc4f31cfecc418b3";

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn pin_for(bytes: &[u8]) -> ArtifactPin {
    ArtifactPin::new(bytes.len() as u64, hex_digest(bytes)).expect("valid pin")
}
/// Deterministic synthetic package with the real public identity but tiny
/// artifact bodies, so tests exercise the exact production URL layout.
struct SyntheticPackage {
    weights: Vec<u8>,
    config: Vec<u8>,
    tokenizer: Vec<u8>,
    package: DownloadDescriptor,
}
fn synthetic_package() -> SyntheticPackage {
    let weights: Vec<u8> = (0u32..7_000).map(|index| (index % 253) as u8).collect();
    let config: Vec<u8> = (0u32..300).map(|index| (index % 251) as u8).collect();
    let tokenizer: Vec<u8> = (0u32..1_500).map(|index| (index % 249) as u8).collect();
    let descriptor = ModelPackageDescriptor {
        weights: pin_for(&weights),
        config: pin_for(&config),
        tokenizer: pin_for(&tokenizer),
    };
    SyntheticPackage {
        weights,
        config,
        tokenizer,
        package: DownloadDescriptor {
            model_id: TEST_MODEL_ID.into(),
            revision: TEST_REVISION.into(),
            descriptor,
        },
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
    fn path_for(&self, kind: ModelArtifactKind) -> String {
        let file = match kind {
            ModelArtifactKind::Weights => WEIGHTS_PATH,
            ModelArtifactKind::Config => CONFIG_PATH,
            ModelArtifactKind::Tokenizer => TOKENIZER_PATH,
        };
        format!(
            "/{}/resolve/{}/{}",
            self.package.model_id, self.package.revision, file
        )
    }
}
struct TestDir(PathBuf);
impl TestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "proteus-download-test-{}-{}-{name}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir(&path).expect("test dir");
        Self(path)
    }
    fn cache(&self) -> std::path::PathBuf {
        self.0.join("cache")
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn budget() -> DownloadBudget {
    DownloadBudget {
        connect_timeout: Duration::from_secs(2),
        request_timeout: Duration::from_secs(2),
        total_timeout: Duration::from_secs(15),
    }
}
fn install_from(
    server: &TinyServer,
    package: &SyntheticPackage,
    cache: &std::path::Path,
    check: &dyn Fn() -> Result<(), String>,
    on_progress: Option<&mut dyn FnMut(InstallProgress)>,
) -> Result<ModelPackagePaths, DownloadError> {
    install_via_endpoint(
        server.url(),
        &package.package,
        cache,
        budget(),
        check,
        on_progress,
    )
}
/// Serves `routes` over one tiny sequential HTTP/1.1 server per test.
#[derive(Clone, Default)]
struct Route {
    status: u16,
    location: Option<String>,
    content_length: Option<usize>,
    body: Vec<u8>,
    /// Send only the first bytes of the body, then close (interrupted stream).
    truncate_after: Option<usize>,
    /// Send the body in chunks of the given size with a delay between chunks.
    drip: Option<(Duration, usize)>,
}
fn ok(body: &[u8]) -> Route {
    Route {
        status: 200,
        content_length: Some(body.len()),
        body: body.to_vec(),
        ..Route::default()
    }
}
fn without_length(body: &[u8]) -> Route {
    Route {
        status: 200,
        body: body.to_vec(),
        ..Route::default()
    }
}
fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        302 => "Found",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "",
    }
}
struct ServerCounters {
    hits: AtomicUsize,
    per_path: Mutex<HashMap<String, usize>>,
}
struct TinyServer {
    url: String,
    stop: Arc<AtomicBool>,
    counters: Arc<ServerCounters>,
    thread: Option<JoinHandle<()>>,
}
impl TinyServer {
    fn start(routes: HashMap<String, Route>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
        let address = listener.local_addr().expect("server address");
        let stop = Arc::new(AtomicBool::new(false));
        let counters = Arc::new(ServerCounters {
            hits: AtomicUsize::new(0),
            per_path: Mutex::new(HashMap::new()),
        });
        let thread_stop = stop.clone();
        let thread_counters = counters.clone();
        let routes = Arc::new(routes);
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if thread_stop.load(Ordering::SeqCst) {
                    break;
                }
                match stream {
                    Ok(stream) => {
                        let routes = routes.clone();
                        let counters = thread_counters.clone();
                        std::thread::spawn(move || handle_connection(stream, routes, counters));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url: format!("http://{address}"),
            stop,
            counters,
            thread: Some(thread),
        }
    }
    fn url(&self) -> &str {
        &self.url
    }
    fn hits(&self) -> usize {
        self.counters.hits.load(Ordering::SeqCst)
    }
    fn hits_for(&self, path: &str) -> usize {
        self.counters
            .per_path
            .lock()
            .unwrap()
            .get(path)
            .copied()
            .unwrap_or(0)
    }
}
impl Drop for TinyServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop so it can observe the stop flag.
        let _ = TcpStream::connect(self.url.trim_start_matches("http://"));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn handle_connection(
    mut stream: TcpStream,
    routes: Arc<HashMap<String, Route>>,
    counters: Arc<ServerCounters>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Some(path) = read_request_path(&mut stream) else {
        return;
    };
    counters.hits.fetch_add(1, Ordering::SeqCst);
    *counters
        .per_path
        .lock()
        .unwrap()
        .entry(path.clone())
        .or_insert(0) += 1;
    let route = routes.get(&path).cloned().unwrap_or_else(|| Route {
        status: 404,
        body: b"not found".to_vec(),
        ..Route::default()
    });
    write_response(&mut stream, &route);
}
fn read_request_path(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];
    loop {
        if head.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if head.len() > 65_536 {
            return None;
        }
        match stream.read(&mut chunk) {
            Ok(0) => return None,
            Ok(read) => head.extend_from_slice(&chunk[..read]),
            Err(_) => return None,
        }
    }
    let text = String::from_utf8_lossy(&head);
    let request_line = text.lines().next()?;
    let path = request_line.split_whitespace().nth(1)?;
    Some(path.to_string())
}
fn write_response(stream: &mut TcpStream, route: &Route) {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nconnection: close\r\n",
        route.status,
        reason_phrase(route.status)
    );
    if let Some(location) = &route.location {
        head.push_str(&format!("location: {location}\r\n"));
    }
    if let Some(length) = route.content_length {
        head.push_str(&format!("content-length: {length}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let body = &route.body;
    match route.truncate_after {
        Some(limit) => {
            let end = limit.min(body.len());
            let _ = stream.write_all(&body[..end]);
        }
        None => match route.drip {
            Some((delay, size)) => {
                for part in body.chunks(size) {
                    std::thread::sleep(delay);
                    if stream.write_all(part).is_err() {
                        break;
                    }
                }
            }
            None => {
                let _ = stream.write_all(body);
            }
        },
    }
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}
fn full_routes(package: &SyntheticPackage) -> HashMap<String, Route> {
    let mut routes = HashMap::new();
    for kind in [
        ModelArtifactKind::Weights,
        ModelArtifactKind::Config,
        ModelArtifactKind::Tokenizer,
    ] {
        routes.insert(package.path_for(kind), ok(package.bytes_for(kind)));
    }
    routes
}
#[test]
fn installs_exact_package_with_factual_progress_and_verified_paths() {
    let server = TinyServer::start(full_routes(&synthetic_package()));
    let dir = TestDir::new("success");
    let package = synthetic_package();
    let progress = RefCell::new(Vec::new());
    let mut sink = |item: InstallProgress| progress.borrow_mut().push(item);
    let paths = install_from(&server, &package, &dir.cache(), &|| Ok(()), Some(&mut sink))
        .expect("package installs");
    assert_eq!(
        *progress.borrow(),
        vec![
            InstallProgress {
                artifacts_verified: 1,
                bytes_verified: package.weights.len() as u64,
            },
            InstallProgress {
                artifacts_verified: 2,
                bytes_verified: (package.weights.len() + package.config.len()) as u64,
            },
            InstallProgress {
                artifacts_verified: 3,
                bytes_verified: (package.weights.len()
                    + package.config.len()
                    + package.tokenizer.len()) as u64,
            },
        ]
    );
    for (kind, path) in [
        (ModelArtifactKind::Weights, &paths.weights),
        (ModelArtifactKind::Config, &paths.config),
        (ModelArtifactKind::Tokenizer, &paths.tokenizer),
    ] {
        let pin = match kind {
            ModelArtifactKind::Weights => &package.package.descriptor.weights,
            ModelArtifactKind::Config => &package.package.descriptor.config,
            ModelArtifactKind::Tokenizer => &package.package.descriptor.tokenizer,
        };
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(pin.sha256.as_str()),
            "cache paths stay content-addressed by the pin"
        );
        assert_eq!(
            fs::read(path).expect("artifact bytes").as_slice(),
            package.bytes_for(kind),
            "returned paths name fully verified bytes"
        );
    }
}
#[test]
fn verified_cache_is_reused_without_any_http_request() {
    let dir = TestDir::new("reuse");
    let package = synthetic_package();
    fs::create_dir_all(dir.cache()).unwrap();
    for kind in [
        ModelArtifactKind::Weights,
        ModelArtifactKind::Config,
        ModelArtifactKind::Tokenizer,
    ] {
        let bytes = package.bytes_for(kind);
        fs::write(dir.cache().join(hex_digest(bytes)), bytes).unwrap();
    }
    let server = TinyServer::start(full_routes(&package));
    install_from(&server, &package, &dir.cache(), &|| Ok(()), None).expect("reuse installs");
    assert_eq!(
        server.hits(),
        0,
        "verified artifacts must never be re-fetched"
    );
}
#[test]
fn corrupt_existing_artifact_errors_and_stays_untouched() {
    let dir = TestDir::new("corrupt");
    let package = synthetic_package();
    fs::create_dir_all(dir.cache()).unwrap();
    let mut corrupt = package.weights.clone();
    corrupt[0] ^= 0xFF;
    fs::write(
        dir.cache().join(&package.package.descriptor.weights.sha256),
        &corrupt,
    )
    .unwrap();
    let server = TinyServer::start(full_routes(&package));
    let error = install_from(&server, &package, &dir.cache(), &|| Ok(()), None)
        .expect_err("corrupt cache entry is rejected");
    assert!(
        matches!(
            error,
            DownloadError::Cache(CacheInstallError::ExistingArtifactCorrupt)
        ),
        "{error:?}"
    );
    assert_eq!(
        fs::read(dir.cache().join(&package.package.descriptor.weights.sha256)).unwrap(),
        corrupt,
        "corrupt existing bytes are preserved for the owning manager"
    );
    assert_eq!(
        server.hits(),
        0,
        "corrupt cache entries never trigger a fetch"
    );
}
#[test]
fn interrupted_stream_preserves_verified_artifacts_and_retry_completes() {
    let dir = TestDir::new("interrupted");
    let package = synthetic_package();
    let mut failing = full_routes(&package);
    failing.insert(
        package.path_for(ModelArtifactKind::Config),
        Route {
            status: 200,
            content_length: Some(package.config.len()),
            body: package.config.clone(),
            truncate_after: Some(120),
            ..Route::default()
        },
    );
    let first_server = TinyServer::start(failing);
    let error = install_from(&first_server, &package, &dir.cache(), &|| Ok(()), None)
        .expect_err("interrupted body fails the install");
    assert!(
        matches!(
            error,
            DownloadError::Cache(CacheInstallError::SourceFailed(_))
        ),
        "{error:?}"
    );
    assert_eq!(
        fs::read(dir.cache().join(&package.package.descriptor.weights.sha256))
            .expect("verified weights retained"),
        package.weights,
        "an earlier verified artifact survives the interruption"
    );
    assert!(
        !dir.cache()
            .join(&package.package.descriptor.config.sha256)
            .exists(),
        "no partial config artifact is published"
    );
    // Retry against a healthy server: only config and tokenizer are fetched.
    let retry_server = TinyServer::start(full_routes(&package));
    install_from(&retry_server, &package, &dir.cache(), &|| Ok(()), None)
        .expect("resumable retry completes");
    assert_eq!(
        retry_server.hits_for(&package.path_for(ModelArtifactKind::Weights)),
        0
    );
    assert_eq!(
        retry_server.hits_for(&package.path_for(ModelArtifactKind::Config)),
        1
    );
    assert_eq!(
        retry_server.hits_for(&package.path_for(ModelArtifactKind::Tokenizer)),
        1
    );
}
#[test]
fn declared_length_mismatch_fails_before_caching() {
    let dir = TestDir::new("length-mismatch");
    let package = synthetic_package();
    let mut routes = full_routes(&package);
    let mut lying = ok(&package.weights);
    lying.content_length = Some(package.weights.len() + 7);
    routes.insert(package.path_for(ModelArtifactKind::Weights), lying);
    let server = TinyServer::start(routes);
    let error = install_from(&server, &package, &dir.cache(), &|| Ok(()), None)
        .expect_err("declared length mismatch aborts");
    match error {
        DownloadError::LengthMismatch { declared, expected } => {
            assert_eq!(declared, package.weights.len() as u64 + 7);
            assert_eq!(expected, package.weights.len() as u64);
        }
        other => panic!("expected LengthMismatch, got {other:?}"),
    }
    assert!(
        !dir.cache().exists() || fs::read_dir(dir.cache()).unwrap().next().is_none(),
        "nothing is cached when the declared length disagrees with the pin"
    );
}
#[test]
fn checksum_mismatch_stays_owned_by_the_cache_verifier() {
    let dir = TestDir::new("checksum-mismatch");
    let package = synthetic_package();
    let mut routes = full_routes(&package);
    let wrong = vec![0xABu8; package.config.len()];
    routes.insert(package.path_for(ModelArtifactKind::Config), ok(&wrong));
    let server = TinyServer::start(routes);
    let error = install_from(&server, &package, &dir.cache(), &|| Ok(()), None)
        .expect_err("wrong bytes fail verification");
    assert!(
        matches!(
            error,
            DownloadError::Cache(CacheInstallError::ArtifactChecksumMismatch)
        ),
        "exact byte verification stays in the cache owner: {error:?}"
    );
}
#[test]
fn oversized_body_is_rejected_by_the_cache_byte_cap() {
    let dir = TestDir::new("oversized");
    let package = synthetic_package();
    let mut routes = full_routes(&package);
    let mut oversized = package.tokenizer.clone();
    oversized.extend_from_slice(&[0xCD; 200]);
    routes.insert(
        package.path_for(ModelArtifactKind::Tokenizer),
        without_length(&oversized),
    );
    let server = TinyServer::start(routes);
    let error = install_from(&server, &package, &dir.cache(), &|| Ok(()), None)
        .expect_err("oversized body fails");
    assert!(
        matches!(
            error,
            DownloadError::Cache(CacheInstallError::ArtifactOversized)
        ),
        "{error:?}"
    );
}
#[test]
fn missing_content_length_still_installs_when_bytes_are_exact() {
    let dir = TestDir::new("no-content-length");
    let package = synthetic_package();
    let mut routes = full_routes(&package);
    routes.insert(
        package.path_for(ModelArtifactKind::Config),
        without_length(&package.config),
    );
    let server = TinyServer::start(routes);
    install_from(&server, &package, &dir.cache(), &|| Ok(()), None)
        .expect("missing Content-Length defers to exact cache verification");
}
#[test]
fn failed_http_status_is_explicit() {
    let dir = TestDir::new("status");
    let package = synthetic_package();
    let mut routes = full_routes(&package);
    routes.insert(
        package.path_for(ModelArtifactKind::Weights),
        Route {
            status: 404,
            body: b"missing".to_vec(),
            ..Route::default()
        },
    );
    let server = TinyServer::start(routes);
    let error = install_from(&server, &package, &dir.cache(), &|| Ok(()), None)
        .expect_err("404 is a typed failure");
    assert!(
        matches!(error, DownloadError::StatusNotSuccess(404)),
        "{error:?}"
    );
}
#[test]
fn bounded_redirect_is_followed_and_counted() {
    let dir = TestDir::new("redirect");
    let package = synthetic_package();
    let mut routes = full_routes(&package);
    let cdn_path = "/cdn/weights.bin".to_string();
    routes.insert(
        package.path_for(ModelArtifactKind::Weights),
        Route {
            status: 302,
            location: Some(cdn_path.clone()),
            ..Route::default()
        },
    );
    routes.insert(cdn_path.clone(), ok(&package.weights));
    let server = TinyServer::start(routes);
    install_from(&server, &package, &dir.cache(), &|| Ok(()), None)
        .expect("one bounded HTTPS-hop redirect succeeds");
    assert_eq!(
        server.hits_for(&package.path_for(ModelArtifactKind::Weights)),
        1
    );
    assert_eq!(server.hits_for(&cdn_path), 1);
}
#[test]
fn redirect_loop_is_rejected_by_the_bounded_policy() {
    let dir = TestDir::new("redirect-loop");
    let package = synthetic_package();
    let mut routes = full_routes(&package);
    let loop_path = package.path_for(ModelArtifactKind::Config);
    routes.insert(
        loop_path.clone(),
        Route {
            status: 302,
            location: Some(loop_path.clone()),
            ..Route::default()
        },
    );
    let server = TinyServer::start(routes);
    let error = install_from(&server, &package, &dir.cache(), &|| Ok(()), None)
        .expect_err("redirect loop never installs");
    assert!(
        matches!(error, DownloadError::RedirectRejected),
        "{error:?}"
    );
    assert!(
        server.hits_for(&loop_path) > MAX_REDIRECTS,
        "the redirect bound is actually enforced: {} hits",
        server.hits_for(&loop_path)
    );
}
#[test]
fn production_client_refuses_cleartext_http_before_any_io() {
    let server = TinyServer::start(HashMap::new());
    let client = client_for(&Endpoint::Production, &budget()).expect("production client");
    let error = client
        .get(format!("{}/x", server.url()))
        .send()
        .expect_err("https_only refuses the URL before connecting");
    assert!(
        matches!(map_request_error(error), DownloadError::NotHttps),
        "the production client is HTTPS-only"
    );
    assert_eq!(server.hits(), 0, "refusal happens before any network IO");
}
#[test]
fn production_urls_are_fixed_https_derived_from_validated_pins() {
    let package = synthetic_package();
    for (kind, file) in [
        (ModelArtifactKind::Weights, "model.safetensors"),
        (ModelArtifactKind::Config, "config.json"),
        (ModelArtifactKind::Tokenizer, "onnx/tokenizer.json"),
    ] {
        let url = artifact_url(&Endpoint::Production, &package.package, kind);
        assert_eq!(
            url,
            format!("https://huggingface.co/{TEST_MODEL_ID}/resolve/{TEST_REVISION}/{file}")
        );
    }
}
#[test]
fn total_deadline_aborts_a_slow_download() {
    let dir = TestDir::new("deadline");
    let package = synthetic_package();
    let mut routes = full_routes(&package);
    let mut slow = ok(&package.weights);
    slow.drip = Some((Duration::from_millis(10), 64));
    routes.insert(package.path_for(ModelArtifactKind::Weights), slow);
    let server = TinyServer::start(routes);
    let tight = DownloadBudget {
        total_timeout: Duration::from_millis(150),
        ..budget()
    };
    let error = install_via_endpoint(
        server.url(),
        &package.package,
        &dir.cache(),
        tight,
        &|| Ok(()),
        None,
    )
    .expect_err("total time budget aborts the install");
    assert!(
        matches!(error, DownloadError::DeadlineExceeded),
        "{error:?}"
    );
}
#[test]
fn host_cancellation_preserves_progress_and_retry_reuses_it() {
    let dir = TestDir::new("cancel");
    let package = synthetic_package();
    let server = TinyServer::start(full_routes(&package));
    let cancel = AtomicBool::new(false);
    let mut on_progress = |progress: InstallProgress| {
        if progress.artifacts_verified == 1 {
            cancel.store(true, Ordering::SeqCst);
        }
    };
    let check = || -> Result<(), String> {
        if cancel.load(Ordering::SeqCst) {
            Err("user cancelled".into())
        } else {
            Ok(())
        }
    };
    let error = install_from(
        &server,
        &package,
        &dir.cache(),
        &check,
        Some(&mut on_progress),
    )
    .expect_err("host cancellation aborts");
    assert!(
        matches!(error, DownloadError::Cancelled(ref reason) if reason == "user cancelled"),
        "{error:?}"
    );
    assert_eq!(
        fs::read(dir.cache().join(&package.package.descriptor.weights.sha256))
            .expect("verified weights retained after cancel"),
        package.weights
    );
    cancel.store(false, Ordering::SeqCst);
    let retry_server = TinyServer::start(full_routes(&package));
    install_from(&retry_server, &package, &dir.cache(), &check, None)
        .expect("retry completes after cancellation");
    assert_eq!(
        retry_server.hits_for(&package.path_for(ModelArtifactKind::Weights)),
        0,
        "verified weights are reused after cancellation"
    );
}
#[test]
fn cancellation_from_final_progress_never_reports_success() {
    let dir = TestDir::new("last-boundary");
    let package = synthetic_package();
    let server = TinyServer::start(full_routes(&package));
    let cancel = AtomicBool::new(false);
    let mut on_progress = |progress: InstallProgress| {
        if progress.artifacts_verified == 3 {
            cancel.store(true, Ordering::SeqCst);
        }
    };
    let check = || -> Result<(), String> {
        if cancel.load(Ordering::SeqCst) {
            Err("cancelled at the last boundary".into())
        } else {
            Ok(())
        }
    };
    install_from(
        &server,
        &package,
        &dir.cache(),
        &check,
        Some(&mut on_progress),
    )
    .expect_err("a cancellation raised from the final progress callback wins over success");
}
/// Deterministic single-thread proof that the manager composition never keeps
/// two source readers (HTTP bodies) live at once: readers are opened and
/// dropped strictly in sequence on this thread.
struct CountingReader<'a> {
    inner: Box<dyn Read>,
    live: &'a Cell<usize>,
}
impl Read for CountingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buffer)
    }
}
impl Drop for CountingReader<'_> {
    fn drop(&mut self) {
        self.live.set(self.live.get() - 1);
    }
}
#[test]
fn at_most_one_source_reader_is_live_at_a_time() {
    let dir = TestDir::new("one-reader");
    let package = synthetic_package();
    let server = TinyServer::start(full_routes(&package));
    let endpoint = Endpoint::Local(server.url().to_string());
    let client = client_for(&endpoint, &budget()).expect("client");
    let live = Cell::new(0usize);
    let max_live = Cell::new(0usize);
    let mut open_source = |kind: ModelArtifactKind| -> std::io::Result<Box<dyn Read + '_>> {
        let reader = open_artifact(&client, &endpoint, &package.package, kind)
            .map_err(|error| std::io::Error::new(ErrorKind::InvalidData, error))?;
        live.set(live.get() + 1);
        max_live.set(max_live.get().max(live.get()));
        Ok(Box::new(CountingReader {
            inner: reader,
            live: &live,
        }))
    };
    crate::local_embedding::manager::install_package(
        &dir.cache(),
        &package.package.descriptor,
        &mut open_source,
        &|| Ok(()),
        None,
    )
    .expect("installs through the counting reader");
    assert_eq!(live.get(), 0, "every opened reader is dropped");
    assert_eq!(
        max_live.get(),
        1,
        "the manager holds at most one HTTP body at a time"
    );
}
#[test]
fn invalid_budget_is_rejected_before_any_io() {
    let server = TinyServer::start(full_routes(&synthetic_package()));
    let dir = TestDir::new("invalid-budget");
    let package = synthetic_package();
    for bad in [
        DownloadBudget {
            connect_timeout: Duration::ZERO,
            ..budget()
        },
        DownloadBudget {
            request_timeout: Duration::ZERO,
            ..budget()
        },
        DownloadBudget {
            total_timeout: Duration::ZERO,
            ..budget()
        },
        DownloadBudget {
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(1),
            ..budget()
        },
    ] {
        let error = install_via_endpoint(
            server.url(),
            &package.package,
            &dir.cache(),
            bad,
            &|| Ok(()),
            None,
        )
        .expect_err("structurally invalid budget is rejected");
        assert!(matches!(error, DownloadError::InvalidBudget), "{error:?}");
    }
    assert_eq!(server.hits(), 0, "budget validation happens before any IO");
}
#[test]
fn invalid_package_identity_is_rejected_before_any_io() {
    let server = TinyServer::start(full_routes(&synthetic_package()));
    let dir = TestDir::new("invalid-package");
    let package = synthetic_package();
    for (model_id, revision) in [
        ("intfloat/multilingual-e5-small", "short"),
        ("intfloat/multilingual-e5-small", &"z".repeat(40)),
        ("evil?query/x", TEST_REVISION),
        ("a//b", TEST_REVISION),
        ("../escape", TEST_REVISION),
    ] {
        let tampered = DownloadDescriptor {
            model_id: model_id.into(),
            revision: revision.to_string(),
            descriptor: package.package.descriptor.clone(),
        };
        let error = install_via_endpoint(
            server.url(),
            &tampered,
            &dir.cache(),
            budget(),
            &|| Ok(()),
            None,
        )
        .expect_err("invalid public identity is rejected");
        assert!(
            matches!(error, DownloadError::InvalidPackage),
            "{model_id} / {revision}: {error:?}"
        );
    }
    assert_eq!(
        server.hits(),
        0,
        "identity validation happens before any IO"
    );
}
#[test]
fn host_cancel_before_opening_makes_no_request() {
    let server = TinyServer::start(full_routes(&synthetic_package()));
    let dir = TestDir::new("cancel-first");
    let package = synthetic_package();
    let error = install_from(
        &server,
        &package,
        &dir.cache(),
        &|| Err("stop".into()),
        None,
    )
    .expect_err("cancellation before opening wins");
    assert!(
        matches!(error, DownloadError::Cancelled(ref reason) if reason == "stop"),
        "{error:?}"
    );
    assert_eq!(server.hits(), 0);
}
#[test]
fn host_cancellation_with_deadline_sentinel_text_stays_a_cancellation() {
    let dir = TestDir::new("sentinel-cancel");
    let package = synthetic_package();
    let server = TinyServer::start(full_routes(&package));
    let cancel = AtomicBool::new(false);
    let mut on_progress = |progress: InstallProgress| {
        if progress.artifacts_verified == 1 {
            cancel.store(true, Ordering::SeqCst);
        }
    };
    // The host supplies an arbitrary reason string; even one byte-identical to
    // the internal deadline marker must stay a host cancellation with its
    // exact text. The failure is raised from the manager's mid-install guard,
    // not from the first preflight check, so it crosses the whole composition.
    let check = || -> Result<(), String> {
        if cancel.load(Ordering::SeqCst) {
            Err(DEADLINE_REASON.to_string())
        } else {
            Ok(())
        }
    };
    let error = install_from(
        &server,
        &package,
        &dir.cache(),
        &check,
        Some(&mut on_progress),
    )
    .expect_err("host cancellation aborts the install");
    assert!(
        matches!(error, DownloadError::Cancelled(ref reason) if reason == DEADLINE_REASON),
        "a host reason equal to the deadline marker must not read as DeadlineExceeded: {error:?}"
    );
    assert_eq!(
        fs::read(dir.cache().join(&package.package.descriptor.weights.sha256))
            .expect("first artifact was verified before the cancellation"),
        package.weights
    );
}
#[test]
fn stalled_response_body_times_out_the_read_and_verifies_nothing() {
    let dir = TestDir::new("stalled-body");
    let package = synthetic_package();
    let mut routes = full_routes(&package);
    // Deliberate simulated slow network on the server side: the headers arrive
    // at once, the first body byte lands far after the per-read budget. There
    // is no sleep on the test thread itself.
    let mut stalled = ok(&package.weights);
    stalled.drip = Some((Duration::from_millis(500), 1024));
    routes.insert(package.path_for(ModelArtifactKind::Weights), stalled);
    let server = TinyServer::start(routes);
    let read_budget = DownloadBudget {
        connect_timeout: Duration::from_millis(50),
        request_timeout: Duration::from_millis(100),
        total_timeout: Duration::from_secs(15),
    };
    let error = install_via_endpoint(
        server.url(),
        &package.package,
        &dir.cache(),
        read_budget,
        &|| Ok(()),
        None,
    )
    .expect_err("a stalled body aborts the install");
    assert!(
        matches!(error, DownloadError::RequestTimeout),
        "the bounded read timed out, not the total deadline: {error:?}"
    );
    assert!(
        !dir.cache()
            .join(&package.package.descriptor.weights.sha256)
            .exists(),
        "a timed-out body never publishes a verified file"
    );
}

#[test]
fn raw_source_errors_preserve_the_original_os_error() {
    let original = io::Error::from_raw_os_error(13);
    let expected_kind = original.kind();
    let expected_text = original.to_string();
    match recover_source_error(original) {
        DownloadError::Cache(CacheInstallError::SourceFailed(error)) => {
            assert_eq!(error.raw_os_error(), Some(13));
            assert_eq!(error.kind(), expected_kind);
            assert_eq!(error.to_string(), expected_text);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
