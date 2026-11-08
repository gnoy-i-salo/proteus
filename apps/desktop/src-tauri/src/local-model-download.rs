//! Stream the native host's checked model package from public Hugging Face URLs.
//! The host supplies pins from `download_descriptor()`; this transport validates
//! their structure and derives fixed artifact paths. It exposes no frontend URL API.
//! Exact length/hash verification and resumable publication stay with the cache.
//!
//! Requests start at Hugging Face and follow at most five HTTPS redirects. No
//! credentials, cookies or auth headers are supplied; environment proxies are ignored.
//! Cancellation and the total deadline are cooperative between bounded cache reads.
//! A blocked read waits for reqwest's per-operation timeout. Streaming one artifact
//! through the cache buffer does not impose an OS-level RSS cap. Cleartext loopback
//! endpoints exist only in tests.
use std::cell::Cell;
use std::fmt;
use std::io::{self, ErrorKind, Read};
use std::path::Path;
use std::time::{Duration, Instant};

use super::cache::CacheInstallError;
use super::manager::{self as manager, InstallProgress, ModelArtifactKind, ModelPackagePaths};
use super::runtime::DownloadDescriptor;

const PRODUCTION_BASE_URL: &str = "https://huggingface.co";
const WEIGHTS_PATH: &str = "model.safetensors";
const CONFIG_PATH: &str = "config.json";
const TOKENIZER_PATH: &str = "onnx/tokenizer.json";
const MAX_REDIRECTS: usize = 5;
/// Internal marker carried through the manager's `Cancelled(String)` channel
/// when the total deadline fires. Classification relies on the transport's own
/// guard state, never on string identity, so a host reason equal to this
/// marker still reports as [`DownloadError::Cancelled`] with its exact text.
const DEADLINE_REASON: &str = "proteus-model-download-deadline";

/// Time bounds enforced by the transport, all controlled by the native host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownloadBudget {
    /// Bound for the connect phase of each request.
    pub connect_timeout: Duration,
    /// Bound for each read/write operation of a request (reqwest semantics).
    pub request_timeout: Duration,
    /// Bound for the whole install across all three artifacts.
    pub total_timeout: Duration,
}
impl DownloadBudget {
    fn validate(&self) -> Result<(), DownloadError> {
        if self.connect_timeout.is_zero()
            || self.request_timeout.is_zero()
            || self.total_timeout.is_zero()
            || self.connect_timeout > self.request_timeout
        {
            return Err(DownloadError::InvalidBudget);
        }
        Ok(())
    }
}
/// Transport failures omit URLs; host cancellation reasons retain their exact text.
#[derive(Debug)]
pub enum DownloadError {
    /// The descriptor is not a structurally valid public package identity.
    InvalidPackage,
    /// The budget is structurally invalid (a zero timeout or connect above request).
    InvalidBudget,
    /// The host cancellation callback aborted the install.
    Cancelled(String),
    /// The install exceeded its total time budget.
    DeadlineExceeded,
    /// The TCP/TLS connect phase failed.
    ConnectFailed,
    /// A bounded read or write operation timed out.
    RequestTimeout,
    /// The redirect policy refused a hop (loop or refused by the bound).
    RedirectRejected,
    /// The HTTPS-only client refused a non-HTTPS URL.
    NotHttps,
    /// The host answered with a non-success status.
    StatusNotSuccess(u16),
    /// The declared body length disagrees with the pinned length.
    LengthMismatch { declared: u64, expected: u64 },
    /// Any other transport failure, reported without internal detail.
    RequestFailed,
    /// The verified cache install rejected the stream (typed cache reason).
    Cache(CacheInstallError),
}
impl fmt::Display for DownloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPackage => {
                write!(
                    formatter,
                    "pinned download descriptor is not a valid public package identity"
                )
            }
            Self::InvalidBudget => write!(formatter, "download budget is structurally invalid"),
            Self::Cancelled(reason) => write!(formatter, "download cancelled: {reason}"),
            Self::DeadlineExceeded => {
                write!(formatter, "download exceeded its total time budget")
            }
            Self::ConnectFailed => write!(formatter, "could not connect to the public model host"),
            Self::RequestTimeout => write!(formatter, "a download read or write timed out"),
            Self::RedirectRejected => {
                write!(formatter, "the public host redirect policy refused a hop")
            }
            Self::NotHttps => write!(formatter, "refused a non-HTTPS download URL"),
            Self::StatusNotSuccess(status) => {
                write!(formatter, "model host returned HTTP status {status}")
            }
            Self::LengthMismatch { declared, expected } => write!(
                formatter,
                "declared body length {declared} does not match the pinned length {expected}"
            ),
            Self::RequestFailed => write!(formatter, "download request failed"),
            Self::Cache(error) => write!(formatter, "verified cache install failed: {error}"),
        }
    }
}
impl std::error::Error for DownloadError {}

enum Endpoint {
    Production,
    #[cfg(test)]
    Local(String),
}
impl Endpoint {
    fn base_url(&self) -> &str {
        match self {
            Self::Production => PRODUCTION_BASE_URL,
            #[cfg(test)]
            Self::Local(base) => base,
        }
    }
    fn https_only(&self) -> bool {
        match self {
            Self::Production => true,
            #[cfg(test)]
            Self::Local(_) => false,
        }
    }
}
/// Structural re-validation of the checked descriptor before it is ever put
/// into a URL. The descriptor is constructed only by the pin owner, which
/// already validates it; this keeps the transport safe even if that owner
/// changes, and makes URL injection structurally impossible.
fn validate_identity(package: &DownloadDescriptor) -> Result<(), DownloadError> {
    let id = &package.model_id;
    let valid_id = !id.is_empty()
        && id.len() <= 200
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
        && id.matches('/').count() == 1
        && !id
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..");
    let valid_revision = package.revision.len() == 40
        && package
            .revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit());
    if !valid_id || !valid_revision {
        return Err(DownloadError::InvalidPackage);
    }
    Ok(())
}
fn artifact_url(
    endpoint: &Endpoint,
    package: &DownloadDescriptor,
    kind: ModelArtifactKind,
) -> String {
    let file = match kind {
        ModelArtifactKind::Weights => WEIGHTS_PATH,
        ModelArtifactKind::Config => CONFIG_PATH,
        ModelArtifactKind::Tokenizer => TOKENIZER_PATH,
    };
    format!(
        "{}/{}/resolve/{}/{}",
        endpoint.base_url(),
        package.model_id,
        package.revision,
        file
    )
}
fn client_for(
    endpoint: &Endpoint,
    budget: &DownloadBudget,
) -> Result<reqwest::blocking::Client, DownloadError> {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .https_only(endpoint.https_only())
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .connect_timeout(budget.connect_timeout)
        .timeout(budget.request_timeout)
        .build()
        .map_err(|_| DownloadError::ConnectFailed)
}
fn map_request_error(error: reqwest::Error) -> DownloadError {
    // With https_only enabled the only builder error at send time is the
    // scheme refusal for a non-HTTPS URL.
    if error.is_builder() {
        return DownloadError::NotHttps;
    }
    if error.is_redirect() {
        return DownloadError::RedirectRejected;
    }
    if error.is_timeout() {
        return DownloadError::RequestTimeout;
    }
    if let Some(status) = error.status() {
        return DownloadError::StatusNotSuccess(status.as_u16());
    }
    if error.is_connect() {
        return DownloadError::ConnectFailed;
    }
    DownloadError::RequestFailed
}
/// Opens one pinned artifact as a streaming reader: fixed URL, success status
/// required, declared length checked against the pin before any body byte is
/// handed to the cache. No checksum is computed here on purpose.
fn open_artifact(
    client: &reqwest::blocking::Client,
    endpoint: &Endpoint,
    package: &DownloadDescriptor,
    kind: ModelArtifactKind,
) -> Result<Box<dyn Read>, DownloadError> {
    let url = artifact_url(endpoint, package, kind);
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .header(reqwest::header::USER_AGENT, "PROTEUS-model-download/1")
        .send()
        .map_err(map_request_error)?;
    let status = response.status();
    if !status.is_success() {
        return Err(DownloadError::StatusNotSuccess(status.as_u16()));
    }
    let expected = match kind {
        ModelArtifactKind::Weights => package.descriptor.weights.bytes,
        ModelArtifactKind::Config => package.descriptor.config.bytes,
        ModelArtifactKind::Tokenizer => package.descriptor.tokenizer.bytes,
    };
    if let Some(declared) = response.content_length() {
        if declared != expected {
            return Err(DownloadError::LengthMismatch { declared, expected });
        }
    }
    Ok(Box::new(response))
}
fn map_install_error(error: CacheInstallError, deadline_hit: &Cell<bool>) -> DownloadError {
    match error {
        // The manager's `Cancelled` channel carries both host reasons and our
        // deadline marker; only an error raised after our own guard observed
        // the total deadline classifies as `DeadlineExceeded`. A host reason
        // equal to the marker string stays a host cancellation verbatim.
        CacheInstallError::Cancelled(_) if deadline_hit.get() => DownloadError::DeadlineExceeded,
        CacheInstallError::Cancelled(reason) => DownloadError::Cancelled(reason),
        CacheInstallError::SourceFailed(io_error) => recover_source_error(io_error),
        other => DownloadError::Cache(other),
    }
}
/// The manager's source channel is `io::Result`; our typed open errors cross
/// it inside the io error and are recovered here. Raw source errors keep
/// their original kind; a raw read/write timeout (a reqwest payload on the io
/// error, per [`DownloadBudget::request_timeout`]) types as
/// [`DownloadError::RequestTimeout`], matching open-time timeout mapping.
fn recover_source_error(io_error: io::Error) -> DownloadError {
    if io_error
        .get_ref()
        .and_then(|payload| payload.downcast_ref::<reqwest::Error>())
        .is_some_and(reqwest::Error::is_timeout)
    {
        return DownloadError::RequestTimeout;
    }
    if !io_error
        .get_ref()
        .is_some_and(|payload| payload.is::<DownloadError>())
    {
        // Keep the original error, including its OS code and payload.
        return DownloadError::Cache(CacheInstallError::SourceFailed(io_error));
    }
    match io_error
        .into_inner()
        .and_then(|payload| payload.downcast::<DownloadError>().ok())
    {
        Some(typed) => *typed,
        None => DownloadError::RequestFailed,
    }
}

fn install(
    endpoint: Endpoint,
    package: &DownloadDescriptor,
    cache_dir: &Path,
    budget: DownloadBudget,
    check: &dyn Fn() -> Result<(), String>,
    on_progress: Option<&mut dyn FnMut(InstallProgress)>,
) -> Result<ModelPackagePaths, DownloadError> {
    validate_identity(package)?;
    budget.validate()?;
    check().map_err(DownloadError::Cancelled)?;
    let started = Instant::now();
    // Only this guard's deadline branch sets the flag, so classification in
    // `map_install_error` is by internal cause, never by matching a host
    // reason string against the marker.
    let deadline_hit = Cell::new(false);
    let guard = || -> Result<(), String> {
        check()?;
        if started.elapsed() > budget.total_timeout {
            deadline_hit.set(true);
            return Err(DEADLINE_REASON.to_string());
        }
        Ok(())
    };
    let client = client_for(&endpoint, &budget)?;
    let mut open_source = |kind: ModelArtifactKind| -> io::Result<Box<dyn Read>> {
        open_artifact(&client, &endpoint, package, kind)
            .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
    };
    manager::install_package(
        cache_dir,
        &package.descriptor,
        &mut open_source,
        &guard,
        on_progress,
    )
    .map_err(|error| map_install_error(error, &deadline_hit))
}
/// Ensures the pinned package under `cache_dir`, fetching only artifacts that
/// are missing from the verified cache through the fixed public HTTPS
/// endpoint. The host owns the cache directory, the time budget and the
/// cancellation callback; already verified cache bytes are reused without any
/// HTTP request and corrupt existing bytes error out untouched.
pub fn install_pinned_package(
    package: &DownloadDescriptor,
    cache_dir: &Path,
    budget: DownloadBudget,
    check: &dyn Fn() -> Result<(), String>,
    on_progress: Option<&mut dyn FnMut(InstallProgress)>,
) -> Result<ModelPackagePaths, DownloadError> {
    install(
        Endpoint::Production,
        package,
        cache_dir,
        budget,
        check,
        on_progress,
    )
}
#[cfg(test)]
fn install_via_endpoint(
    base_url: &str,
    package: &DownloadDescriptor,
    cache_dir: &Path,
    budget: DownloadBudget,
    check: &dyn Fn() -> Result<(), String>,
    on_progress: Option<&mut dyn FnMut(InstallProgress)>,
) -> Result<ModelPackagePaths, DownloadError> {
    install(
        Endpoint::Local(base_url.to_string()),
        package,
        cache_dir,
        budget,
        check,
        on_progress,
    )
}
#[cfg(test)]
#[path = "local-model-download-tests.rs"]
mod tests;
