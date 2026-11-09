//! Read-only history commands; callers choose an opaque ID, never a path.
use super::{SavedRunPage, SavedRunText};
use tauri::Manager;
fn directory(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("experiments"))
        .map_err(|_| "Каталог истории запусков недоступен.".into())
}
#[tauri::command]
pub async fn list_saved_experiments(
    app: tauri::AppHandle,
    offset: usize,
    limit: usize,
) -> Result<SavedRunPage, String> {
    let base = directory(&app)?;
    tauri::async_runtime::spawn_blocking(move || super::list_saved_runs(&base, offset, limit))
        .await
        .map_err(|_| "Не удалось прочитать список запусков.".to_string())?
}
#[tauri::command]
pub async fn read_saved_experiment(
    app: tauri::AppHandle,
    id: String,
) -> Result<SavedRunText, String> {
    let base = directory(&app)?;
    tauri::async_runtime::spawn_blocking(move || super::read_saved_run(&base, &id))
        .await
        .map_err(|_| "Не удалось прочитать сохранённый запуск.".to_string())?
}
