//! Native owner of the pinned package and one shared CPU runtime. The host
//! supplies the cache directory and authorization checks; neither comes from a model.
use super::{
    cache::CacheInstallError,
    download::{self, DownloadBudget, DownloadError},
    manager::{self, InstallProgress, PackageStatus},
    model::{model_error, EmbeddingModel},
    runtime::{self, DownloadDescriptor, E5Runtime, EmbeddingBudget, ModelFiles},
    slot::{CachedRuntime, CancelToken, RuntimeSlot, SlotError},
};
use serde::Serialize;
use std::{
    path::{Component, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelState {
    pub schema_version: u8,
    pub model_id: String,
    pub model_revision: String,
    pub package_bytes: u64,
    pub runtime_loaded: bool,
    pub operation_active: bool,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PackageState {
    Missing,
    Ready,
}
pub struct LocalModelService {
    cache_dir: PathBuf,
    package: DownloadDescriptor,
    runtime: RuntimeSlot<E5Runtime>,
    stopped: AtomicBool,
}
impl LocalModelService {
    pub fn new(cache_dir: PathBuf) -> Result<Self, String> {
        if !cache_dir.is_absolute() || cache_dir.components().any(|c| c == Component::ParentDir) {
            return Err("Каталог модели должен задаваться native-приложением.".into());
        }
        Ok(Self {
            cache_dir,
            package: runtime::download_descriptor().map_err(model_error)?,
            runtime: RuntimeSlot::new(runtime::RESIDENT_RESERVATION_BYTES).map_err(slot_error)?,
            stopped: AtomicBool::new(false),
        })
    }
    pub fn state(&self) -> ModelState {
        let state = self.runtime.status();
        ModelState {
            schema_version: 1,
            model_id: self.package.model_id.clone(),
            model_revision: self.package.revision.clone(),
            package_bytes: self.package_bytes(),
            runtime_loaded: state.cached_runtime == CachedRuntime::Ready,
            operation_active: state.operation_active,
        }
    }
    pub fn inspect(
        &self,
        operation_id: &str,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<PackageState, String> {
        let started = Instant::now();
        self.runtime
            .maintain(operation_id, |token| -> Result<_, String> {
                let guard = || {
                    operation_check(
                        &self.stopped,
                        token,
                        started,
                        Duration::from_secs(60),
                        check,
                    )
                };
                let status = manager::inspect_package_status(
                    &self.cache_dir,
                    &self.package.descriptor,
                    &guard,
                )
                .map_err(cache_error)?;
                guard()?;
                Ok(match status {
                    PackageStatus::Missing => PackageState::Missing,
                    PackageStatus::Ready { .. } => PackageState::Ready,
                })
            })
            .map_err(slot_error)
    }
    pub fn download(
        &self,
        operation_id: &str,
        check: &dyn Fn() -> Result<(), String>,
        progress: &mut dyn FnMut(InstallProgress),
    ) -> Result<InstallProgress, String> {
        let started = Instant::now();
        let duration = Duration::from_secs(1800);
        self.runtime
            .maintain(operation_id, |token| -> Result<_, String> {
                let guard = || operation_check(&self.stopped, token, started, duration, check);
                guard()?;
                download::install_pinned_package(
                    &self.package,
                    &self.cache_dir,
                    DownloadBudget {
                        connect_timeout: Duration::from_secs(10),
                        request_timeout: Duration::from_secs(30),
                        total_timeout: duration,
                    },
                    &guard,
                    Some(progress),
                )
                .map_err(|error| match error {
                    DownloadError::Cancelled(reason) => reason,
                    _ => "Не удалось загрузить и проверить локальную модель.".into(),
                })?;
                guard()?;
                Ok(InstallProgress {
                    artifacts_verified: 3,
                    bytes_verified: self.package_bytes(),
                })
            })
            .map_err(slot_error)
    }
    pub fn with_runtime<R>(
        &self,
        operation_id: &str,
        budget: EmbeddingBudget,
        check: &dyn Fn() -> Result<(), String>,
        work: impl FnOnce(&dyn EmbeddingModel, &dyn Fn() -> Result<(), String>) -> Result<R, String>,
    ) -> Result<R, String> {
        budget.validate().map_err(model_error)?;
        let started = Instant::now();
        let guard = |token: &CancelToken| {
            operation_check(
                &self.stopped,
                token,
                started,
                Duration::from_millis(budget.max_duration_ms),
                check,
            )
        };
        self.runtime
            .run(
                operation_id,
                budget.max_resident_bytes,
                |token| {
                    let guard = || guard(token);
                    let PackageStatus::Ready { paths } = manager::inspect_package_status(
                        &self.cache_dir,
                        &self.package.descriptor,
                        &guard,
                    )
                    .map_err(cache_error)?
                    else {
                        return Err("Локальная модель не загружена.".into());
                    };
                    E5Runtime::load(
                        &ModelFiles {
                            weights: paths.weights,
                            config: paths.config,
                            tokenizer: paths.tokenizer,
                        },
                        &budget,
                        &guard,
                    )
                    .map_err(model_error)
                },
                |runtime, token| {
                    let guard = || guard(token);
                    guard()?;
                    let result = work(runtime, &guard)?;
                    guard()?;
                    Ok(result)
                },
            )
            .map_err(slot_error)
    }
    pub fn cancel(&self, operation_id: &str) -> Result<bool, String> {
        self.runtime.cancel(operation_id).map_err(slot_error)
    }
    pub fn unload(&self) -> Result<bool, String> {
        self.runtime.unload().map_err(slot_error)
    }
    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
    }
    fn package_bytes(&self) -> u64 {
        let package = &self.package.descriptor;
        package.weights.bytes + package.config.bytes + package.tokenizer.bytes
    }
}
fn operation_check(
    stopped: &AtomicBool,
    token: &CancelToken,
    started: Instant,
    duration: Duration,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    if stopped.load(Ordering::Acquire) {
        return Err("Приложение завершает локальную обработку.".into());
    }
    if token.is_cancelled() {
        return Err("Локальная обработка отменена.".into());
    }
    if started.elapsed() >= duration {
        return Err("Превышено время локальной обработки.".into());
    }
    check()
}
fn cache_error(error: CacheInstallError) -> String {
    match error {
        CacheInstallError::Cancelled(reason) => reason,
        _ => "Файлы локальной модели повреждены или недоступны.".into(),
    }
}
fn slot_error<E: ToString>(error: SlotError<E>) -> String {
    match error {
        SlotError::LoadFailed(reason) | SlotError::WorkFailed(reason) => reason.to_string(),
        SlotError::Busy => "Локальная модель занята другой операцией.".into(),
        SlotError::Cancelled => "Локальная обработка отменена.".into(),
        _ => "Неверный идентификатор операции или бюджет локальной модели.".into(),
    }
}
#[cfg(test)]
#[path = "local-model-service-tests.rs"]
mod tests;
