#[path = "cli-command.rs"]
mod command;
#[path = "cli-executable.rs"]
mod executable;
#[path = "cli-invocation.rs"]
mod invocation;
#[path = "cli-model.rs"]
pub(crate) mod model;
use executable::executable;
use invocation::execute;
use model::{validate, Event};
pub use model::{Availability, Request, Response};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Emitter, Manager};

#[derive(Default, Clone)]
pub struct Runs(pub Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>);

#[tauri::command]
pub fn cli_status() -> Vec<Availability> {
    ["codex", "kimi"]
        .into_iter()
        .map(|provider| {
            let path = executable(provider);
            Availability {
                provider: provider.into(),
                available: path.is_some(),
                path: path.map(|p| p.to_string_lossy().into_owned()),
            }
        })
        .collect()
}
#[tauri::command]
pub async fn run_cli(
    app: tauri::AppHandle,
    state: tauri::State<'_, Runs>,
    request: Request,
) -> Result<Response, String> {
    validate(&request)?;
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("runs")
        .join(&request.id);
    let runs = state.0.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut active = runs.lock().map_err(|e| e.to_string())?;
        if !active.is_empty() {
            return Err("Another CLI stage is already running".into());
        }
        active.insert(request.id.clone(), cancelled.clone());
    }
    let id = request.id.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        let event_id = request.id.clone();
        let result = execute(
            request,
            &directory,
            cancelled,
            |kind, elapsed_ms, reasoning| {
                let _ = app.emit(
                    "cli-progress",
                    Event {
                        id: event_id.clone(),
                        kind: kind.into(),
                        elapsed_ms,
                        reasoning,
                    },
                );
            },
        );
        // Transient prompts are not retained; completed experiment artifacts are saved separately.
        let _ = std::fs::remove_dir_all(directory);
        result
    })
    .await
    .map_err(|e| e.to_string());
    runs.lock().map_err(|e| e.to_string())?.remove(&id);
    result?
}
#[tauri::command]
pub fn cancel_cli(state: tauri::State<'_, Runs>, id: String) -> Result<(), String> {
    if let Some(flag) = state.0.lock().map_err(|e| e.to_string())?.get(&id) {
        flag.store(true, Ordering::SeqCst);
    }
    Ok(())
}
#[cfg(test)]
#[path = "cli-tests.rs"]
mod tests;
#[cfg(test)]
#[path = "cli-wire-tests.rs"]
mod wire_tests;
