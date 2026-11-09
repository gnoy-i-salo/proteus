//! Local-search IPC only selects an operation; persisted native jobs own its scope.
use super::{job_workflow, search_workflow};
use crate::sources::Sources;

#[tauri::command]
pub async fn rebuild_ingestion_index(
    request: search_workflow::IndexRequest,
    sources: tauri::State<'_, Sources>,
) -> Result<search_workflow::IndexResult, String> {
    request.validate()?;
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let provider = job_workflow::job_provider(&sources, &request.job_id)?;
        let operation = sources.begin_ingestion_operation(&request.operation_id, provider)?;
        search_workflow::rebuild(&sources, &operation, &request, || {
            chrono::Utc::now().timestamp_millis()
        })
    })
    .await
    .map_err(|_| "Не удалось перестроить локальный индекс.".to_string())?
}

#[tauri::command]
pub async fn search_ingestion_corpus(
    request: search_workflow::QueryRequest,
    sources: tauri::State<'_, Sources>,
) -> Result<search_workflow::QueryResult, String> {
    request.validate()?;
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let provider = job_workflow::job_provider(&sources, &request.job_id)?;
        let operation = sources.begin_ingestion_operation(&request.operation_id, provider)?;
        search_workflow::query(&sources, &operation, &request, || {
            chrono::Utc::now().timestamp_millis()
        })
    })
    .await
    .map_err(|_| "Не удалось выполнить локальный поиск.".to_string())?
}

#[tauri::command]
pub async fn inspect_ingestion_episode(
    request: super::inspection::EpisodeInspectRequest,
    sources: tauri::State<'_, Sources>,
) -> Result<super::inspection::EpisodeInspectResult, String> {
    request.validate()?;
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let provider = job_workflow::job_provider(&sources, &request.job_id)?;
        let operation = sources.begin_ingestion_operation(&request.operation_id, provider)?;
        super::inspection::inspect(&sources, &operation, &request, || {
            chrono::Utc::now().timestamp_millis()
        })
    })
    .await
    .map_err(|_| "Не удалось открыть фрагменты источника.".to_string())?
}
