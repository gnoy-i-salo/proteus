//! Model-management IPC exposes fixed native operations, never paths, URLs,
//! arbitrary model profiles or source text for inference.
use super::service::{LocalModelService, ModelState, PackageState};
use serde::Serialize;
use std::sync::Arc;
use tauri::Emitter;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInstallProgress {
    pub schema_version: u8,
    pub operation_id: String,
    pub artifacts_verified: u32,
    pub bytes_verified: u64,
}
#[tauri::command]
pub fn local_model_state(service: tauri::State<'_, Arc<LocalModelService>>) -> ModelState {
    service.state()
}
#[tauri::command]
pub async fn inspect_local_model(
    operation_id: String,
    service: tauri::State<'_, Arc<LocalModelService>>,
) -> Result<PackageState, String> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.inspect(&operation_id, &|| Ok(())))
        .await
        .map_err(|_| "Не удалось проверить файлы модели.".to_string())?
}
#[tauri::command]
pub async fn download_local_model(
    app: tauri::AppHandle,
    operation_id: String,
    service: tauri::State<'_, Arc<LocalModelService>>,
) -> Result<ModelInstallProgress, String> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let event = |progress: super::manager::InstallProgress| ModelInstallProgress {
            schema_version: 1,
            operation_id: operation_id.clone(),
            artifacts_verified: progress.artifacts_verified,
            bytes_verified: progress.bytes_verified,
        };
        let progress = service.download(&operation_id, &|| Ok(()), &mut |progress| {
            let _ = app.emit("local-model-progress", event(progress));
        })?;
        Ok(event(progress))
    })
    .await
    .map_err(|_| "Не удалось завершить загрузку модели.".to_string())?
}
#[tauri::command]
pub fn cancel_local_model_operation(
    operation_id: String,
    service: tauri::State<'_, Arc<LocalModelService>>,
) -> Result<bool, String> {
    service.cancel(&operation_id)
}
#[tauri::command]
pub async fn unload_local_model(
    service: tauri::State<'_, Arc<LocalModelService>>,
) -> Result<bool, String> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.unload())
        .await
        .map_err(|_| "Не удалось освободить память модели.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_progress_matches_the_frontend_wire_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../src/shared/lib/fixtures/local-model-wire.json"
        ))
        .unwrap();
        let package = super::super::runtime::cache_descriptor().unwrap();
        let progress = ModelInstallProgress {
            schema_version: 1,
            operation_id: "download-fixture".into(),
            artifacts_verified: 3,
            bytes_verified: package.weights.bytes + package.config.bytes + package.tokenizer.bytes,
        };
        assert_eq!(serde_json::to_value(progress).unwrap(), fixture["progress"]);
        assert_eq!(serde_json::to_value(PackageState::Ready).unwrap(), "ready");
        assert_eq!(
            serde_json::to_value(PackageState::Missing).unwrap(),
            "missing"
        );
    }
}
