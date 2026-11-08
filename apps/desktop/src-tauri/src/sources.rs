#[path = "source-binding.rs"]
mod binding;
#[path = "source-binding-policy.rs"]
mod binding_policy;
pub use binding::SourceBindingResult;
pub use binding_policy::{CreatedSourceBinding, SourceBindingRequest};
pub(crate) use binding_policy::{DocumentRelation, RelationKind, SourceAuthor, SpeakerRelation};
#[path = "source-collection.rs"]
mod collection;
#[path = "source-corpus.rs"]
mod corpus;
#[path = "source-document.rs"]
mod document;
#[path = "source-grant.rs"]
mod grant;
#[path = "source-import.rs"]
mod import;
#[path = "source-ingestion.rs"]
mod ingestion;
#[path = "source-model.rs"]
mod model;
#[path = "source-readers.rs"]
mod readers;
#[path = "source-storage-codec.rs"]
mod storage_codec;
#[path = "source-store.rs"]
mod store;
use crate::connections::Provider;
pub use corpus::Sources;
pub use model::{ImportedSource, SourceAction, SourceGrant, SourceGrantRequest, SourceResult};
use std::sync::atomic::Ordering;
use tauri_plugin_dialog::DialogExt;
#[tauri::command]
pub async fn create_source_grant(
    request: SourceGrantRequest,
    sources: tauri::State<'_, Sources>,
) -> Result<SourceGrant, String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sources.create(request))
        .await
        .map_err(|_| "Не удалось создать разрешение источника.".to_string())?
}
#[tauri::command]
pub async fn source_action(
    grant_id: String,
    action: SourceAction,
    sources: tauri::State<'_, Sources>,
) -> Result<SourceResult, String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sources.execute(&grant_id, action))
        .await
        .map_err(|_| "Не удалось выполнить действие источника.".to_string())?
}
#[tauri::command]
pub async fn cancel_source(
    grant_id: String,
    sources: tauri::State<'_, Sources>,
) -> Result<(), String> {
    let sources = sources.inner().clone();
    sources.flag(&grant_id)?.store(true, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || sources.cancel(&grant_id, true))
        .await
        .map_err(|_| "Не удалось отменить сбор.".to_string())?
}
#[tauri::command]
pub async fn resume_source(
    grant_id: String,
    sources: tauri::State<'_, Sources>,
) -> Result<(), String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sources.cancel(&grant_id, false))
        .await
        .map_err(|_| "Не удалось возобновить сбор.".to_string())?
}
#[tauri::command]
pub async fn reserve_model_step(
    grant_id: String,
    sources: tauri::State<'_, Sources>,
) -> Result<(), String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sources.reserve_model_step(&grant_id))
        .await
        .map_err(|_| "Не удалось зарезервировать шаг модели.".to_string())?
}
#[tauri::command]
pub async fn clear_source_data(
    provider: Provider,
    sources: tauri::State<'_, Sources>,
) -> Result<(), String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sources.clear(provider))
        .await
        .map_err(|_| "Не удалось удалить данные источника.".to_string())?
}
#[tauri::command]
pub async fn delete_source(
    grant_id: String,
    sources: tauri::State<'_, Sources>,
) -> Result<(), String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sources.delete(&grant_id))
        .await
        .map_err(|_| "Не удалось удалить выбранный корпус.".to_string())?
}
#[tauri::command]
pub async fn import_source(
    app: tauri::AppHandle,
    provider: Provider,
    sources: tauri::State<'_, Sources>,
) -> Result<Option<ImportedSource>, String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .add_filter(
                "Экспорт сообщений или выбранный текст",
                &["json", "txt", "md"],
            )
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        sources
            .import(
                provider,
                &file.into_path().map_err(|_| "Нужен локальный файл.")?,
            )
            .map(Some)
    })
    .await
    .map_err(|_| "Не удалось импортировать источник.".to_string())?
}

#[tauri::command]
pub async fn create_source_binding(
    request: SourceBindingRequest,
    sources: tauri::State<'_, Sources>,
) -> Result<CreatedSourceBinding, String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sources.create_binding(request))
        .await
        .map_err(|_| "Не удалось создать привязку источника.".to_string())?
}
#[tauri::command]
pub async fn source_binding_action(
    binding_id: String,
    action: SourceAction,
    sources: tauri::State<'_, Sources>,
) -> Result<SourceBindingResult, String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || sources.binding_action(&binding_id, action))
        .await
        .map_err(|_| "Не удалось прочитать выбранный контекст.".to_string())?
}
#[cfg(test)]
#[path = "source-tests.rs"]
mod tests;
#[cfg(test)]
#[path = "source-wire-tests.rs"]
mod wire_tests;
