//! Floppy Station app: the Tauri shell around the core library. Runs in the
//! tray; the window and the loading box are created only when needed.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod events;
mod locale;
mod tray;
mod view;
mod watcher;
mod windows;

use tauri::{Manager, RunEvent};

use tray::Tray;
use watcher::Watcher;
use windows::LoadingBox;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let app = tauri::Builder::default()
        // Starting the app again opens the running one's window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::open_main(app);
        }))
        .setup(|app| {
            app.manage(LoadingBox::default());
            let watcher = Watcher::start(app.handle().clone())?;
            let tray = Tray::create(app.handle(), &watcher.line(), watcher.detection())?;
            app.manage(tray);
            // Catch up with what happened while the tray was being created.
            app.state::<Tray>()
                .update(&watcher.line(), watcher.detection());
            app.manage(watcher);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            watcher::recent_events,
            watcher::app_state,
            watcher::app_version,
            watcher::get_settings,
            watcher::save_settings,
            watcher::set_detection,
            watcher::removable_drives,
            watcher::loading_box_state,
            watcher::close_loading_box,
            watcher::open_main_window,
        ])
        .build(tauri::generate_context!())
        .expect("error while starting Floppy Station");

    app.run(|_app, event| {
        // Closing the last window keeps the app in the tray; only Exit in
        // the tray menu (which gives an exit code) quits.
        if let RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
        }
    });
}
