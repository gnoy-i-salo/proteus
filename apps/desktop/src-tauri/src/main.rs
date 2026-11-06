#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
#[path = "cli-protocol.rs"]
mod cli_protocol;
mod presets;
use std::sync::atomic::Ordering;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            presets::queue_files(
                app,
                presets::argument_paths(args, std::path::Path::new(&cwd)),
            );
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(presets::PendingPresets::default())
        .setup(|app| {
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
            cli::save_experiment,
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
