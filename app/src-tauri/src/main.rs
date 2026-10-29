//! Floppy Station app: the Tauri shell around the core library. Runs in the
//! tray; the window and the loading box are created only when needed.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod disk;
mod events;
mod locale;
mod system;
mod tray;
mod view;
mod watcher;
mod windows;

use tauri::{Manager, RunEvent};

use tray::Tray;
use watcher::Watcher;
use windows::{LoadingBox, MainTab};

fn main() {
    // Started by the drive monitor to wait for a disk: see the core's
    // `floppy::wait` module.
    if floppy_station_core::floppy::run_wait_helper() {
        return;
    }
    system::init_logging();
    log::info!("Floppy Station {} starting", env!("CARGO_PKG_VERSION"));

    let app = tauri::Builder::default()
        // Starting the app again opens the running one's window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::open_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            app.manage(LoadingBox::default());
            app.manage(MainTab::default());
            let watcher = Watcher::start(app.handle().clone())?;
            let tray = Tray::create(app.handle(), &watcher.line(), watcher.detection())?;
            app.manage(tray);
            // Catch up with what happened while the tray was being created.
            app.state::<Tray>()
                .update(&watcher.line(), watcher.detection());
            let first_run = watcher.state().first_run;
            app.manage(watcher);
            // The app starts in the tray, except the very first time, when
            // the window welcomes the user.
            if first_run {
                windows::open_main(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            watcher::recent_events,
            watcher::app_state,
            watcher::app_version,
            watcher::get_settings,
            watcher::default_settings,
            watcher::save_settings,
            watcher::set_detection,
            watcher::finish_welcome,
            watcher::removable_drives,
            watcher::loading_box_state,
            watcher::close_loading_box,
            watcher::open_main_window,
            watcher::take_main_tab,
            watcher::ui_language,
            disk::steam_games,
            disk::open_applications,
            disk::pick_program,
            disk::config_preview,
            disk::write_config,
            disk::remove_config,
            disk::approvals,
            disk::approve_disk,
            disk::revoke_approval,
            system::startup_enabled,
            system::set_startup,
            system::open_log_folder,
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
