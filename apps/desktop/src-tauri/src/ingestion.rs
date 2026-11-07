#[path = "ingestion-input.rs"]
mod input;
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
