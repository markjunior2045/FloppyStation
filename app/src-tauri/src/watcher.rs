//! Runs the core library inside the app: watches the drive, applies the game
//! rules and passes every event to the UI, the tray and the loading box.

use std::collections::VecDeque;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use floppy_station_core::controller::{Controller, GameEvent, System};
use floppy_station_core::diskconfig::DiskContent;
use floppy_station_core::floppy::{
    self, DriveLetter, FloppyEvent, InsertOrigin, Monitor, Poller, ProbeKind,
};
use floppy_station_core::launcher::LaunchError;
use floppy_station_core::settings::Settings;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::events::{self, AppEvent};
use crate::tray::Tray;
use crate::view::{AppState, DriveView};
use crate::windows::{self, BoxState, LoadingBox};

/// Events kept for a window opened later.
const RECENT_EVENTS: usize = 200;
/// Name of the Tauri event carrying an [`AppEvent`].
const EVENT_NAME: &str = "app-event";
/// Name of the Tauri event carrying the [`AppState`].
const STATE_NAME: &str = "app-state";
/// How long the loading box stays after a game is launched: the app doesn't
/// know when the game finishes opening.
const BOX_AFTER_LAUNCH: Duration = Duration::from_secs(3);
/// How long the loading box shows a message.
const BOX_MESSAGE: Duration = Duration::from_secs(10);

/// Watches the drive while detection is on. Cheap to clone.
#[derive(Clone)]
pub struct Watcher {
    inner: Arc<Inner>,
}

struct Inner {
    app: AppHandle,
    settings: Mutex<Settings>,
    /// Settings changed since the controller last handled an event.
    controller_settings: Mutex<Option<Settings>>,
    /// Problem loading the settings at startup, as a code.
    settings_error: Option<&'static str>,
    detection: AtomicBool,
    /// Present while detection is on; dropping it stops checking the drive.
    monitor: Mutex<Option<Monitor>>,
    /// Kept so the controller outlives each monitor.
    floppy_tx: Sender<FloppyEvent>,
    log: Mutex<EventLog>,
    view: Mutex<DriveView>,
}

#[derive(Default)]
struct EventLog {
    next_seq: u64,
    recent: VecDeque<AppEvent>,
}

impl Watcher {
    /// Loads the settings and, if detection is on, starts watching the
    /// drive.
    pub fn start(app: AppHandle) -> Result<Self, Box<dyn Error>> {
        let (settings, error) = Settings::load_or_default();
        let (floppy_tx, floppy_rx) = mpsc::channel();
        let (game_tx, game_rx) = mpsc::channel();
        let watcher = Self {
            inner: Arc::new(Inner {
                app,
                detection: AtomicBool::new(settings.detection_enabled),
                view: Mutex::new(DriveView::new(settings.drive, settings.detection_enabled)),
                settings: Mutex::new(settings.clone()),
                controller_settings: Mutex::new(None),
                settings_error: error.as_ref().map(|error| error.code()),
                monitor: Mutex::new(None),
                floppy_tx,
                log: Mutex::new(EventLog::default()),
            }),
        };
        if let Some(error) = error {
            log::warn!("using default settings ({}): {error}", error.code());
            watcher.record(events::from_settings_error(&error));
        }

        // Game events also come from the threads that close games, so they
        // get a thread of their own.
        let this = watcher.clone();
        thread::Builder::new()
            .name("game events".into())
            .spawn(move || {
                for event in game_rx {
                    this.on_game(&event);
                }
            })?;
        let this = watcher.clone();
        thread::Builder::new()
            .name("controller".into())
            .spawn(move || {
                let mut controller = Controller::new(System, &settings, game_tx);
                for event in floppy_rx {
                    // A monitor being stopped may still report a last check.
                    if !this.detection() {
                        continue;
                    }
                    if let Some(settings) = lock(&this.inner.controller_settings).take() {
                        controller.update_settings(&settings);
                    }
                    this.on_floppy(&event);
                    controller.handle(&event);
                }
            })?;

        if watcher.detection() {
            watcher.start_monitor()?;
        } else {
            watcher.record(events::detection(false));
        }
        Ok(watcher)
    }

    pub fn detection(&self) -> bool {
        self.inner.detection.load(Ordering::SeqCst)
    }

    pub fn settings(&self) -> Settings {
        lock(&self.inner.settings).clone()
    }

    /// Turns detection on or off and saves the choice.
    pub fn set_detection(&self, enabled: bool) {
        let settings = Settings {
            detection_enabled: enabled,
            ..self.settings()
        };
        if let Err(code) = self.save_settings(settings) {
            log::warn!("could not save the settings ({code})");
        }
    }

    /// Saves new settings and applies them right away:
    /// - Detection off stops checking the drive. Turned back on, a disk
    ///   already in the drive is treated like one present when the app
    ///   starts: its game isn't launched.
    /// - Another drive starts watching it from scratch.
    /// - New intervals apply from the next check.
    /// - What to do with games applies from the next disk removal.
    pub fn save_settings(&self, mut new: Settings) -> Result<(), &'static str> {
        new.normalize();
        let old = self.settings();
        if new == old {
            return Ok(());
        }
        let path = Settings::default_path().map_err(|error| error.code())?;
        new.save_to(&path).map_err(|error| error.code())?;
        *lock(&self.inner.settings) = new.clone();
        *lock(&self.inner.controller_settings) = Some(new.clone());

        let detection_changed = new.detection_enabled != old.detection_enabled;
        let drive_changed = new.drive != old.drive;
        self.inner
            .detection
            .store(new.detection_enabled, Ordering::SeqCst);
        if detection_changed || drive_changed {
            lock(&self.inner.monitor).take();
            lock(&self.inner.view).reset(new.drive, new.detection_enabled);
            if new.detection_enabled {
                if let Err(error) = self.start_monitor() {
                    log::error!("could not start the drive monitor: {error}");
                }
            } else {
                self.inner.app.state::<LoadingBox>().close(&self.inner.app);
            }
        } else if new.poll_intervals() != old.poll_intervals()
            && let Some(monitor) = lock(&self.inner.monitor).as_ref()
        {
            monitor.set_intervals(new.poll_intervals());
        }

        if detection_changed {
            self.record(events::detection(new.detection_enabled));
        } else {
            self.publish_state();
        }
        Ok(())
    }

    /// The tray line for the current state.
    pub fn line(&self) -> String {
        lock(&self.inner.view).line()
    }

    pub fn state(&self) -> AppState {
        lock(&self.inner.view).state(self.inner.settings_error)
    }

    fn start_monitor(&self) -> std::io::Result<()> {
        let settings = self.settings();
        let poller = Poller::new(
            [settings.drive],
            ProbeKind::default().create(),
            settings.poll_intervals(),
        );
        let monitor = Monitor::spawn(poller, self.inner.floppy_tx.clone())?;
        *lock(&self.inner.monitor) = Some(monitor);
        Ok(())
    }

    fn on_floppy(&self, event: &FloppyEvent) {
        lock(&self.inner.view).on_floppy(event);
        self.record(events::from_floppy(event));
        let app = &self.inner.app;
        let loading = app.state::<LoadingBox>();
        match event {
            FloppyEvent::MediaInserted { origin, .. }
                if *origin != InsertOrigin::PresentAtStartup
                    && self.settings().show_loading_box =>
            {
                loading.show(app, BoxState::READING, None);
            }
            FloppyEvent::MediaRemoved { .. } => loading.close(app),
            _ => {}
        }
    }

    fn on_game(&self, event: &GameEvent) {
        lock(&self.inner.view).on_game(event);
        self.record(events::from_game(event));
        let app = &self.inner.app;
        let loading = app.state::<LoadingBox>();
        // The box only follows a disk it showed up for.
        if !loading.is_open() {
            return;
        }
        let message = |key| loading.show(app, BoxState::message(key), Some(BOX_MESSAGE));
        match event {
            GameEvent::DiskRead { inspection, .. } => match &inspection.content {
                DiskContent::Configured { .. } => {}
                DiskContent::NotConfigured => message("box.blank"),
                DiskContent::UnsupportedVersion(_) => message("box.newer"),
                DiskContent::NoDisk => loading.close(app),
                _ => message("box.unusable"),
            },
            GameEvent::Untrusted { .. } => message("box.untrusted"),
            GameEvent::Launched { .. } => {
                loading.show(app, BoxState::READING, Some(BOX_AFTER_LAUNCH));
            }
            GameEvent::LaunchFailed { error, .. } => match error {
                LaunchError::NotFound => message("box.missing"),
                _ => message("box.failed"),
            },
            GameEvent::Reattached { .. } | GameEvent::StartupDiskIgnored { .. } => {
                loading.close(app);
            }
            _ => {}
        }
    }

    /// Numbers the event, keeps it for windows opened later, sends it to the
    /// open ones and updates the window's state and the tray.
    fn record(&self, mut event: AppEvent) {
        {
            let mut log = lock(&self.inner.log);
            log.next_seq += 1;
            event.seq = log.next_seq;
            if log.recent.len() == RECENT_EVENTS {
                log.recent.pop_front();
            }
            log.recent.push_back(event.clone());
        }
        log::info!("{} {:?}", event.code, event.params);
        if let Err(error) = self.inner.app.emit(EVENT_NAME, &event) {
            log::warn!("could not send {} to the UI: {error}", event.code);
        }
        self.publish_state();
    }

    /// Sends the current state to the window and the tray.
    fn publish_state(&self) {
        let app = &self.inner.app;
        let _ = app.emit(STATE_NAME, self.state());
        // The tray doesn't exist yet while the app starts.
        if let Some(tray) = app.try_state::<Tray>() {
            tray.update(&self.line(), self.detection());
        }
    }

    fn recent(&self) -> Vec<AppEvent> {
        lock(&self.inner.log).recent.iter().cloned().collect()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// Events that happened before the window opened.
#[tauri::command]
pub fn recent_events(watcher: State<'_, Watcher>) -> Vec<AppEvent> {
    watcher.recent()
}

#[tauri::command]
pub fn app_state(watcher: State<'_, Watcher>) -> AppState {
    watcher.state()
}

#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[tauri::command]
pub fn get_settings(watcher: State<'_, Watcher>) -> Settings {
    watcher.settings()
}

/// Saves and applies the settings; fails with a `settings_*` code.
#[tauri::command]
pub fn save_settings(watcher: State<'_, Watcher>, settings: Settings) -> Result<(), &'static str> {
    watcher.save_settings(settings)
}

#[tauri::command]
pub fn set_detection(watcher: State<'_, Watcher>, enabled: bool) {
    watcher.set_detection(enabled);
}

/// The removable drives, plus the watched drive even if it isn't plugged in.
#[tauri::command]
pub fn removable_drives(watcher: State<'_, Watcher>) -> Vec<String> {
    let mut drives = floppy::removable_drives();
    let current: DriveLetter = watcher.settings().drive;
    if !drives.contains(&current) {
        drives.push(current);
        drives.sort();
    }
    drives.iter().map(ToString::to_string).collect()
}

/// What the loading box shows; `None` once it should close.
#[tauri::command]
pub fn loading_box_state(loading: State<'_, LoadingBox>) -> Option<BoxState> {
    loading.current()
}

#[tauri::command]
pub fn close_loading_box(app: AppHandle, loading: State<'_, LoadingBox>) {
    loading.close(&app);
}

/// The loading box's buttons: close it and open the main window.
#[tauri::command]
pub fn open_main_window(app: AppHandle, loading: State<'_, LoadingBox>) {
    loading.close(&app);
    windows::open_main(&app);
}
