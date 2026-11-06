#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
#[path = "cli-protocol.rs"]
mod cli_protocol;
use std::sync::atomic::Ordering;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .manage(cli::Runs::default())
        .invoke_handler(tauri::generate_handler![
            cli::cli_status,
            cli::run_cli,
            cli::cancel_cli,
            cli::save_experiment
        ])
        .build(tauri::generate_context!())
        .expect("failed to build PROTEUS")
        .run(|app, event| {
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
