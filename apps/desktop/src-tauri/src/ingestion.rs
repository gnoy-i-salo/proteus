#[path = "ingestion-fingerprint.rs"]
pub mod fingerprint;
#[path = "ingestion-grants.rs"]
pub mod grants;
#[path = "ingestion-input.rs"]
mod input;
#[path = "ingestion-operations.rs"]
pub mod operations;
#[path = "ingestion-preview.rs"]
pub mod preview;
#[path = "ingestion-preview-scan.rs"]
pub mod preview_scan;
#[path = "ingestion-preview-store.rs"]
pub mod preview_store;
#[path = "ingestion-preview-workflow.rs"]
pub mod preview_workflow;
#[path = "telegram-archive-json.rs"]
pub mod telegram_archive_json;
#[path = "telegram-document.rs"]
pub mod telegram_document;
#[path = "ingestion-telegram-worker.rs"]
pub mod telegram_worker;
pub use input::SelectedInputs;
use serde::Serialize;
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedInput {
    schema_version: u8,
    source_handle: String,
    label: String,
    input_bytes: u64,
}

#[tauri::command]
pub async fn pick_ingestion_source(
    app: tauri::AppHandle,
    inputs: tauri::State<'_, std::sync::Arc<SelectedInputs>>,
) -> Result<Option<SelectedInput>, String> {
    let inputs = inputs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .add_filter(
                "Выбранный архив или документ",
                &["json", "html", "zip", "txt", "md"],
            )
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|_| "Выберите локальный файл.")?;
        let source_handle = crate::connections::storage::random_id(24)?;
        let (label, input_bytes) = inputs.insert(source_handle.clone(), &path)?;
        Ok(Some(SelectedInput {
            schema_version: 1,
            source_handle,
            label,
            input_bytes,
        }))
    })
    .await
    .map_err(|_| "Не удалось выбрать источник загрузки.".to_string())?
}

#[tauri::command]
pub async fn release_ingestion_source(
    source_handle: String,
    inputs: tauri::State<'_, std::sync::Arc<SelectedInputs>>,
) -> Result<(), String> {
    let inputs = inputs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || inputs.release(&source_handle))
        .await
        .map_err(|_| "Не удалось освободить выбранный источник.".to_string())?
}

#[tauri::command]
pub async fn preview_ingestion_source(
    request: preview_workflow::PreviewRequest,
    sources: tauri::State<'_, crate::sources::Sources>,
    inputs: tauri::State<'_, std::sync::Arc<SelectedInputs>>,
) -> Result<preview_workflow::PreviewResult, String> {
    let sources = sources.inner().clone();
    let inputs = inputs.inner().clone();
    let operation = sources.begin_ingestion_operation(
        &request.operation_id,
        crate::corpus_database::JobProvider::Telegram,
    )?;
    tauri::async_runtime::spawn_blocking(move || {
        preview_workflow::preview(&sources, &inputs, &operation, request)
    })
    .await
    .map_err(|_| "Не удалось выполнить предпросмотр источника.".to_string())?
}

#[tauri::command]
pub fn cancel_ingestion_operation(
    operation_id: String,
    sources: tauri::State<'_, crate::sources::Sources>,
) -> Result<(), String> {
    sources.cancel_ingestion_operation(&operation_id)
}
