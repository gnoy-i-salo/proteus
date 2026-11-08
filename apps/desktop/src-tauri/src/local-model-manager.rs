//! Bounded local model-package cache manager foundation for PROTEUS.
//!
//! This module is intentionally a thin orchestration layer over the verified,
//! content-addressed artifact cache owned by `local-embedding-cache.rs`: it
//! adds no hashing, no publication logic and no Candle/runtime dependency of
//! its own. The exact package semantic model fingerprint (runtime manifest
//! interpretation) remains the root runtime's responsibility; this manager
//! only ever sees trusted [`ArtifactPin`] values.
//!
//! The descriptor is trusted root input: this module accepts exactly three
//! artifacts (weights/config/tokenizer) and never downloads on its own, runs
//! no inference and falls back to no cloud provider. Filesystem paths in
//! [`ModelPackagePaths`] are native-only and must not cross the IPC boundary
//! as frontend DTOs.
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::local_embedding::cache::{self as cache, ArtifactPin, CacheInstallError};

/// Role of one artifact inside a pinned local model package.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModelArtifactKind {
    Weights,
    Config,
    Tokenizer,
}
/// Fixed installation order: weights, then config, then tokenizer.
pub const MODEL_ARTIFACT_KINDS: [ModelArtifactKind; 3] = [
    ModelArtifactKind::Weights,
    ModelArtifactKind::Config,
    ModelArtifactKind::Tokenizer,
];

/// Trusted three-artifact descriptor supplied by the owning manager. It is the
/// only package identity this module accepts: no frontend URLs, no caller
/// filenames and no runtime-manifest parsing happen here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelPackageDescriptor {
    pub weights: ArtifactPin,
    pub config: ArtifactPin,
    pub tokenizer: ArtifactPin,
}
impl ModelPackageDescriptor {
    fn pin_for(&self, kind: ModelArtifactKind) -> &ArtifactPin {
        match kind {
            ModelArtifactKind::Weights => &self.weights,
            ModelArtifactKind::Config => &self.config,
            ModelArtifactKind::Tokenizer => &self.tokenizer,
        }
    }
}
/// Native filesystem view of a ready package. These paths stay on the native
/// side and are never serialized into frontend DTOs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelPackagePaths {
    pub weights: PathBuf,
    pub config: PathBuf,
    pub tokenizer: PathBuf,
}
/// Inspection result for a package under a caller-provided cache directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageStatus {
    /// The package is absent or incomplete; a partial package is never ready.
    Missing,
    /// Every artifact was fully re-verified against its pin.
    Ready { paths: ModelPackagePaths },
}
/// Factual install progress: only bytes and artifacts that have actually
/// passed verification are reported; nothing is estimated or invented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstallProgress {
    pub artifacts_verified: u32,
    pub bytes_verified: u64,
}

/// Read-only inspection: reports `PackageStatus::Ready` only when all three
/// artifacts fully re-verify against their pins under `cache_dir`. This never
/// creates the cache directory, never modifies existing bytes and never opens
/// a source reader or network handle. A missing cache root or artifact is
/// `PackageStatus::Missing`; a corrupt, blocked or cancelled lookup is an
/// explicit `CacheInstallError`, so a partial package is never ready.
pub fn inspect_package_status(
    cache_dir: &Path,
    descriptor: &ModelPackageDescriptor,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<PackageStatus, CacheInstallError> {
    let weights = cache::lookup_verified(cache_dir, &descriptor.weights, check)?;
    let config = cache::lookup_verified(cache_dir, &descriptor.config, check)?;
    let tokenizer = cache::lookup_verified(cache_dir, &descriptor.tokenizer, check)?;
    check().map_err(CacheInstallError::Cancelled)?;
    Ok(match (weights, config, tokenizer) {
        (Some(weights), Some(config), Some(tokenizer)) => PackageStatus::Ready {
            paths: ModelPackagePaths {
                weights,
                config,
                tokenizer,
            },
        },
        _ => PackageStatus::Missing,
    })
}
/// Ensures one artifact: reuses and fully re-verifies a verified cache entry
/// without touching the source, otherwise opens the caller's reader lazily and
/// installs through the cache's bounded verifier. The reader is dropped before
/// the next artifact is handled, so at most one source reader is live at a
/// time and verified artifacts are retained for a resumable retry if a later
/// artifact fails.
fn ensure_artifact<'a>(
    cache_dir: &Path,
    kind: ModelArtifactKind,
    descriptor: &ModelPackageDescriptor,
    open_source: &mut dyn FnMut(ModelArtifactKind) -> io::Result<Box<dyn Read + 'a>>,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<PathBuf, CacheInstallError> {
    let pin = descriptor.pin_for(kind);
    if let Some(path) = cache::lookup_verified(cache_dir, pin, check)? {
        return Ok(path);
    }
    check().map_err(CacheInstallError::Cancelled)?;
    let mut reader = open_source(kind).map_err(CacheInstallError::SourceFailed)?;
    cache::install_verified_artifact(cache_dir, pin, &mut *reader, check)
}
/// Installs the missing artifacts of a trusted descriptor and returns the
/// verified native paths of all three artifacts.
///
/// `open_source` is keyed by [`ModelArtifactKind`]; this module never
/// interprets URLs or filenames and opens readers lazily, only for artifacts
/// that are actually missing. Already cached artifacts are fully re-verified
/// and never re-fetched; cancellation, a source failure or a checksum/length
/// mismatch aborts with a typed error, never reports `Ready`, and leaves
/// earlier verified artifacts plus any pre-existing cache bytes untouched.
/// `on_progress`, when supplied, receives only factual counts of artifacts
/// and bytes that have actually passed verification.
///
/// Deliberate limitations (inherited from the cache owner): this protects
/// only against ordinary same-user filesystem races; a malicious local
/// process with identical OS privileges can modify cache contents outside
/// this module, and orphan temporary directories from a killed install are
/// left for the owning lifecycle to remove. No background download, no
/// inference and no cloud fallback happen here.
pub fn install_package<'a>(
    cache_dir: &Path,
    descriptor: &ModelPackageDescriptor,
    open_source: &mut dyn FnMut(ModelArtifactKind) -> io::Result<Box<dyn Read + 'a>>,
    check: &dyn Fn() -> Result<(), String>,
    mut on_progress: Option<&mut dyn FnMut(InstallProgress)>,
) -> Result<ModelPackagePaths, CacheInstallError> {
    check().map_err(CacheInstallError::Cancelled)?;
    let mut progress = InstallProgress {
        artifacts_verified: 0,
        bytes_verified: 0,
    };
    let mut paths = ModelPackagePaths {
        weights: PathBuf::new(),
        config: PathBuf::new(),
        tokenizer: PathBuf::new(),
    };
    for kind in MODEL_ARTIFACT_KINDS {
        let path = ensure_artifact(cache_dir, kind, descriptor, open_source, check)?;
        progress.artifacts_verified += 1;
        progress.bytes_verified += descriptor.pin_for(kind).bytes;
        if let Some(on_progress) = on_progress.as_deref_mut() {
            on_progress(progress);
        }
        match kind {
            ModelArtifactKind::Weights => paths.weights = path,
            ModelArtifactKind::Config => paths.config = path,
            ModelArtifactKind::Tokenizer => paths.tokenizer = path,
        }
    }
    check().map_err(CacheInstallError::Cancelled)?;
    Ok(paths)
}
#[cfg(test)]
#[path = "local-model-manager-tests.rs"]
mod tests;
