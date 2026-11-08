#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[path = "archive-zip.rs"]
pub mod archive_zip;
#[path = "corpus-database.rs"]
pub mod corpus_database;
pub mod ingestion;
#[path = "local-embedding.rs"]
pub mod local_embedding;

mod cli;
#[path = "cli-policy.rs"]
mod cli_policy;
#[path = "cli-protocol.rs"]
mod cli_protocol;
mod connections;
#[path = "experiment-storage.rs"]
mod experiment_storage;
mod presets;
mod sources;
use std::sync::atomic::Ordering;
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;

fn auth_callback(app: &tauri::AppHandle, url: &str) {
    if !url.starts_with("proteus:") {
        return;
    }
    let Some(connections) = app.try_state::<connections::Connections>() else {
        return;
    };
    let connections = connections.inner().clone();
    let app = app.clone();
    let url = url.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let result = connections.handle_callback(&url);
        // Never broadcast the callback URL, authorization code, or token response.
        match result {
            Ok(status) => {
                let _ = app.emit("connection-status", status);
            }
            Err(_) => {
                let _ = app.emit(
                    "connection-error",
                    "Не удалось завершить вход VK. Начните подключение заново в настройках.",
                );
            }
        }
    });
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            presets::queue_files(
                app,
                presets::argument_paths(args, std::path::Path::new(&cwd)),
            );
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(presets::PendingPresets::default())
        .manage(std::sync::Arc::new(ingestion::SelectedInputs::default()))
        .setup(|app| {
            let directory = app.path().app_data_dir()?.join("sources");
            let connections =
                connections::Connections::new(directory.clone()).map_err(std::io::Error::other)?;
            app.manage(sources::Sources::new(
                directory.join("corpus"),
                connections.clone(),
            ));
            app.manage(connections);
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            app.deep_link().register_all()?;
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    auth_callback(&handle, url.as_str());
                }
            });
            if let Some(urls) = app.deep_link().get_current()? {
                for url in urls {
                    auth_callback(app.handle(), url.as_str());
                }
            }
            let cwd = std::env::current_dir()?;
            presets::queue_files(
                app.handle(),
                presets::argument_paths(std::env::args(), &cwd),
            );
            Ok(())
        })
        .manage(cli::Runs::default())
        .invoke_handler(tauri::generate_handler![
            cli::cli_status,
            cli::run_cli,
            cli::cancel_cli,
            experiment_storage::save_experiment,
            connections::configure_connection,
            connections::connection_statuses,
            connections::begin_connection,
            connections::open_connection_page,
            connections::advance_connection,
            connections::disconnect_connection,
            sources::create_source_grant,
            sources::create_source_binding,
            sources::source_binding_action,
            sources::source_action,
            sources::cancel_source,
            sources::resume_source,
            sources::import_source,
            ingestion::pick_ingestion_source,
            ingestion::release_ingestion_source,
            ingestion::preview_ingestion_source,
            ingestion::preview_ingestion_page,
            ingestion::preview_ingestion_url,
            ingestion::cancel_ingestion_operation,
            ingestion::job_commands::create_ingestion_binding,
            ingestion::job_commands::start_ingestion_job,
            ingestion::job_commands::get_ingestion_job,
            ingestion::job_commands::publish_ingestion_job,
            ingestion::job_commands::prepare_ingestion_job,
            ingestion::job_commands::pause_ingestion_job,
            ingestion::job_commands::cancel_ingestion_job,
            ingestion::job_commands::resume_ingestion_job,
            ingestion::job_commands::retry_ingestion_job,
            ingestion::job_commands::restore_ingestion_source,
            sources::reserve_model_step,
            sources::clear_source_data,
            sources::delete_source,
            presets::import_preset,
            presets::export_preset,
            presets::take_pending_presets
        ])
        .build(tauri::generate_context!())
        .expect("failed to build PROTEUS")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { ref urls } = event {
                presets::queue_files(app, urls.iter().filter_map(|url| url.to_file_path().ok()));
            }
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                app.state::<sources::Sources>().shutdown();
                app.state::<connections::Connections>().shutdown();
                let runs = app.state::<cli::Runs>().0.clone();
                if let Ok(active) = runs.lock() {
                    if !active.is_empty() {
                        api.prevent_exit();
                        for flag in active.values() {
                            flag.store(true, Ordering::SeqCst);
                        }
                        let handle = app.clone();
                        let pending = runs.clone();
                        std::thread::spawn(move || {
                            loop {
                                if pending.lock().is_ok_and(|items| items.is_empty()) {
                                    break;
                                }
                                std::thread::sleep(std::time::Duration::from_millis(100));
                            }
                            handle.exit(0);
                        });
                    }
                };
            }
        });
}
