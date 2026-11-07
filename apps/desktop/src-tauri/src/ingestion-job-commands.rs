//! Thin IPC adapters; durable transitions and acquisition belong to the workflow.
use super::{
    grants::BindingRequest,
    job_workflow::{self, Control, JobView, Restart, StartRequest},
    operations::Operation,
    SelectedInputs,
};
use crate::{
    corpus_database::{JobBinding, JobProvider, JobStatus, StoredJob},
    sources::Sources,
};
use serde::Serialize;
use std::sync::Arc;
use tauri::Emitter;
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunError {
    job_id: String,
    message: String,
}

#[tauri::command]
pub async fn create_ingestion_binding(
    request: BindingRequest,
    sources: tauri::State<'_, Sources>,
) -> Result<JobBinding, String> {
    let sources = sources.inner().clone();
    let id = format!("binding:{}", crate::connections::storage::random_id(18)?);
    let operation = sources.begin_ingestion_operation(&id, JobProvider::Telegram)?;
    tauri::async_runtime::spawn_blocking(move || {
        sources.with_ingestion_database(&operation, |db| {
            super::grants::create_binding(db, request, now_ms())
        })
    })
    .await
    .map_err(|_| "Не удалось сохранить разрешение источника.".to_string())?
}
#[tauri::command]
pub async fn start_ingestion_job(
    app: tauri::AppHandle,
    request: StartRequest,
    sources: tauri::State<'_, Sources>,
    inputs: tauri::State<'_, Arc<SelectedInputs>>,
) -> Result<JobView, String> {
    let sources = sources.inner().clone();
    let inputs = inputs.inner().clone();
    let operation =
        sources.begin_ingestion_operation(&request.operation_id, JobProvider::Telegram)?;
    let prepare_sources = sources.clone();
    let prepare_inputs = inputs.clone();
    let (job, operation) = tauri::async_runtime::spawn_blocking(move || {
        let job = job_workflow::create(
            &prepare_sources,
            &prepare_inputs,
            &operation,
            request,
            now_ms(),
        )?;
        Ok::<_, String>((job, operation))
    })
    .await
    .map_err(|_| "Не удалось начать импорт.".to_string())??;
    launch(app, sources, inputs, operation, &job);
    Ok(job.into())
}
#[tauri::command]
pub async fn get_ingestion_job(
    job_id: String,
    sources: tauri::State<'_, Sources>,
) -> Result<JobView, String> {
    let sources = sources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        job_workflow::inspect(&sources, &job_id).map(JobView::from)
    })
    .await
    .map_err(|_| "Не удалось прочитать состояние импорта.".to_string())?
}
#[tauri::command]
pub async fn pause_ingestion_job(
    job_id: String,
    sources: tauri::State<'_, Sources>,
) -> Result<JobView, String> {
    control(sources.inner().clone(), job_id, Control::Pause).await
}
#[tauri::command]
pub async fn cancel_ingestion_job(
    job_id: String,
    sources: tauri::State<'_, Sources>,
) -> Result<JobView, String> {
    control(sources.inner().clone(), job_id, Control::Cancel).await
}
async fn control(sources: Sources, id: String, action: Control) -> Result<JobView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        job_workflow::control(&sources, &id, action).map(JobView::from)
    })
    .await
    .map_err(|_| "Не удалось изменить состояние импорта.".to_string())?
}
#[tauri::command]
pub async fn resume_ingestion_job(
    app: tauri::AppHandle,
    job_id: String,
    sources: tauri::State<'_, Sources>,
    inputs: tauri::State<'_, Arc<SelectedInputs>>,
) -> Result<JobView, String> {
    restart(
        app,
        sources.inner().clone(),
        inputs.inner().clone(),
        job_id,
        Restart::Resume,
    )
    .await
}
#[tauri::command]
pub async fn retry_ingestion_job(
    app: tauri::AppHandle,
    job_id: String,
    sources: tauri::State<'_, Sources>,
    inputs: tauri::State<'_, Arc<SelectedInputs>>,
) -> Result<JobView, String> {
    restart(
        app,
        sources.inner().clone(),
        inputs.inner().clone(),
        job_id,
        Restart::Retry,
    )
    .await
}
async fn restart(
    app: tauri::AppHandle,
    sources: Sources,
    inputs: Arc<SelectedInputs>,
    id: String,
    action: Restart,
) -> Result<JobView, String> {
    let prepare_sources = sources.clone();
    let prepare_inputs = inputs.clone();
    let (job, operation) = tauri::async_runtime::spawn_blocking(move || {
        job_workflow::restart(&prepare_sources, &prepare_inputs, &id, action, now_ms())
    })
    .await
    .map_err(|_| "Не удалось возобновить импорт.".to_string())??;
    launch(app, sources, inputs, operation, &job);
    Ok(job.into())
}
fn launch(
    app: tauri::AppHandle,
    sources: Sources,
    inputs: Arc<SelectedInputs>,
    operation: Operation,
    job: &StoredJob,
) {
    if job.status != JobStatus::Running {
        return;
    }
    let operation = Arc::new(operation);
    let job_id = job.id.clone();
    // Keep the guard in the supervisor too: even a panicking reader must leave
    // durable failure metadata, rather than an unobserved detached task result.
    tauri::async_runtime::spawn(async move {
        let reader_sources = sources.clone();
        let reader_operation = operation.clone();
        let outcome = tauri::async_runtime::spawn_blocking(move || {
            job_workflow::execute(&reader_sources, &inputs, &reader_operation, now_ms)
        })
        .await;
        let result = match outcome {
            Ok(result) => result,
            Err(_) => tauri::async_runtime::spawn_blocking(move || {
                job_workflow::finish_failure(
                    &sources,
                    &operation,
                    super::telegram_worker::Failure {
                        message: "Импорт прерван до завершения. Его можно повторить.".into(),
                        retryable: true,
                    },
                    now_ms(),
                )
            })
            .await
            .unwrap_or_else(|_| Err("Не удалось сохранить состояние прерванного импорта.".into())),
        };
        match result {
            Ok(job) => {
                let _ = app.emit("ingestion-job-updated", JobView::from(job));
            }
            Err(message) => {
                let _ = app.emit("ingestion-job-error", RunError { job_id, message });
            }
        }
    });
}

#[tauri::command]
pub async fn restore_ingestion_source(
    request: super::restore_workflow::RestoreRequest,
    sources: tauri::State<'_, Sources>,
    inputs: tauri::State<'_, Arc<SelectedInputs>>,
) -> Result<JobView, String> {
    let sources = sources.inner().clone();
    let inputs = inputs.inner().clone();
    let operation =
        sources.begin_ingestion_operation(&request.operation_id, JobProvider::Telegram)?;
    tauri::async_runtime::spawn_blocking(move || {
        super::restore_workflow::restore(&sources, &inputs, &operation, request, now_ms)
            .map(JobView::from)
    })
    .await
    .map_err(|_| "Не удалось восстановить выбранный источник.".to_string())?
}
