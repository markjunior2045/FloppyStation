//! Runs the core library inside the app: watches the drive, applies the game
//! rules and records every event for the UI, as the console tool does.

use std::collections::VecDeque;
use std::error::Error;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use floppy_station_core::controller::{Controller, System};
use floppy_station_core::floppy::{DriveLetter, Monitor, Poller, ProbeKind};
use floppy_station_core::settings::Settings;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::events::{self, AppEvent};

/// Events kept for a window opened later.
const RECENT_EVENTS: usize = 200;

/// Name of the Tauri event carrying an [`AppEvent`].
const EVENT_NAME: &str = "app-event";

/// The running drive monitor and the recent events. Dropping it stops the
/// monitor.
pub struct Watcher {
    _monitor: Monitor,
    log: Arc<EventLog>,
    drive: DriveLetter,
}

#[derive(Default)]
struct EventLog {
    state: Mutex<LogState>,
}

#[derive(Default)]
struct LogState {
    next_seq: u64,
    recent: VecDeque<AppEvent>,
}

impl EventLog {
    /// Numbers the event, keeps it for windows opened later and sends it to
    /// the open ones.
    fn record(&self, app: &AppHandle, mut event: AppEvent) {
        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.next_seq += 1;
            event.seq = state.next_seq;
            if state.recent.len() == RECENT_EVENTS {
                state.recent.pop_front();
            }
            state.recent.push_back(event.clone());
        }
        log::info!("{} {:?}", event.code, event.params);
        if let Err(error) = app.emit(EVENT_NAME, &event) {
            log::warn!("could not send {} to the UI: {error}", event.code);
        }
    }

    fn recent(&self) -> Vec<AppEvent> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.recent.iter().cloned().collect()
    }
}

impl Watcher {
    /// Loads the settings and starts watching the drive.
    pub fn start(app: AppHandle) -> Result<Self, Box<dyn Error>> {
        let log = Arc::new(EventLog::default());
        let (settings, error) = Settings::load_or_default();
        if let Some(error) = error {
            log::warn!("using default settings ({}): {error}", error.code());
            log.record(&app, events::from_settings_error(&error));
        }

        let (floppy_tx, floppy_rx) = mpsc::channel();
        let poller = Poller::new(
            [settings.drive],
            ProbeKind::default().create(),
            settings.poll_intervals(),
        );
        let monitor = Monitor::spawn(poller, floppy_tx)?;

        // Game events also come from the threads that close games, so they
        // get a thread of their own.
        let (game_tx, game_rx) = mpsc::channel();
        {
            let (app, log) = (app.clone(), log.clone());
            thread::Builder::new()
                .name("game events".into())
                .spawn(move || {
                    for event in game_rx {
                        log.record(&app, events::from_game(&event));
                    }
                })?;
        }
        {
            let (app, log) = (app.clone(), log.clone());
            let settings = settings.clone();
            thread::Builder::new()
                .name("controller".into())
                .spawn(move || {
                    let mut controller = Controller::new(System, &settings, game_tx);
                    for event in floppy_rx {
                        log.record(&app, events::from_floppy(&event));
                        controller.handle(&event);
                    }
                })?;
        }

        Ok(Self {
            _monitor: monitor,
            log,
            drive: settings.drive,
        })
    }
}

/// Events that happened before the window opened.
#[tauri::command]
pub fn recent_events(watcher: State<'_, Watcher>) -> Vec<AppEvent> {
    watcher.log.recent()
}

#[derive(Serialize)]
pub struct AppInfo {
    drive: String,
    version: &'static str,
}

#[tauri::command]
pub fn app_info(watcher: State<'_, Watcher>) -> AppInfo {
    AppInfo {
        drive: watcher.drive.to_string(),
        version: env!("CARGO_PKG_VERSION"),
    }
}
