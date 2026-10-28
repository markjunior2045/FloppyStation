//! Floppy Station app: the Tauri shell around the core library. Starts the
//! drive monitor and the game rules, and passes what happens to the UI.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod events;
mod watcher;

use tauri::Manager;

use watcher::Watcher;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .setup(|app| {
            let watcher = Watcher::start(app.handle().clone())?;
            app.manage(watcher);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            watcher::recent_events,
            watcher::app_info
        ])
        .run(tauri::generate_context!())
        .expect("error while running Floppy Station");
}
