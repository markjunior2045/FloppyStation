//! The app's windows. Both are created when needed and destroyed when
//! closed, so no WebView2 runs while the app sits in the tray.

use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// Label of the main window.
pub const MAIN: &str = "main";
/// Label of the loading box.
pub const LOADING: &str = "loading";

/// Tabs of the main window that other places can open it on.
pub const TABS: &[&str] = &["status", "disk", "approvals", "prefs"];

/// The tab a main window being created should open on.
#[derive(Default)]
pub struct MainTab(Mutex<Option<&'static str>>);

impl MainTab {
    /// The tab to open on, once.
    pub fn take(&self) -> Option<&'static str> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }
}

/// Opens the main window on `tab` (one of [`TABS`]), or switches the open
/// window to it.
pub fn open_main_on(app: &AppHandle, tab: &'static str) {
    if app.get_webview_window(MAIN).is_some() {
        let _ = app.emit_to(MAIN, "open-tab", tab);
    } else {
        *app.state::<MainTab>()
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(tab);
    }
    open_main(app);
}

/// Opens the main window, or brings it to the front if it is open. Only
/// called on the user's request, or to welcome them on the first run: the
/// app never opens windows by itself otherwise.
pub fn open_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    // Building a window from an event handler can deadlock on Windows.
    let app = app.clone();
    thread::spawn(move || {
        let result = WebviewWindowBuilder::new(&app, MAIN, WebviewUrl::App("index.html".into()))
            .title("Floppy Station")
            .inner_size(760.0, 520.0)
            .resizable(false)
            .maximizable(false)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .center()
            .build();
        if let Err(error) = result {
            log::error!("could not open the window: {error}");
        }
    });
}

/// What the loading box shows: the progress bar while reading, or a message
/// (a key of the language file).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BoxState {
    pub message: Option<&'static str>,
}

impl BoxState {
    pub const READING: Self = Self { message: None };

    pub fn message(key: &'static str) -> Self {
        Self { message: Some(key) }
    }
}

/// Name of the Tauri event carrying the loading box's [`BoxState`].
const BOX_EVENT: &str = "loading-box";

/// The small box shown in the middle of the screen while a disk is read. It
/// stays on top without taking focus, so a full screen game keeps it.
#[derive(Default)]
pub struct LoadingBox {
    inner: Mutex<BoxInner>,
}

#[derive(Default)]
struct BoxInner {
    /// Changes on every show and close, so a delayed close only closes the
    /// box it was meant for.
    generation: u64,
    current: Option<BoxState>,
    /// Whether the window exists or is being built. A window being built
    /// reads `current` once it loads.
    window: bool,
}

impl LoadingBox {
    pub fn current(&self) -> Option<BoxState> {
        self.lock().current
    }

    pub fn is_open(&self) -> bool {
        self.lock().current.is_some()
    }

    /// Shows `state`, opening the box if needed, and closes it after
    /// `close_after` if nothing changes in the meantime.
    pub fn show(&self, app: &AppHandle, state: BoxState, close_after: Option<Duration>) {
        let (generation, needs_window) = {
            let mut inner = self.lock();
            inner.generation += 1;
            inner.current = Some(state);
            let needs_window = !inner.window;
            inner.window = true;
            (inner.generation, needs_window)
        };
        if needs_window {
            self.build(app);
        } else {
            let _ = app.emit_to(LOADING, BOX_EVENT, state);
        }
        if let Some(delay) = close_after {
            let app = app.clone();
            thread::spawn(move || {
                thread::sleep(delay);
                app.state::<LoadingBox>().close_if(&app, generation);
            });
        }
    }

    pub fn close(&self, app: &AppHandle) {
        let generation = self.lock().generation;
        self.close_if(app, generation);
    }

    fn close_if(&self, app: &AppHandle, generation: u64) {
        {
            let mut inner = self.lock();
            if inner.generation != generation {
                return;
            }
            inner.generation += 1;
            inner.current = None;
            inner.window = false;
        }
        if let Some(window) = app.get_webview_window(LOADING) {
            let _ = window.destroy();
        }
    }

    fn build(&self, app: &AppHandle) {
        let app = app.clone();
        thread::spawn(move || {
            let result =
                WebviewWindowBuilder::new(&app, LOADING, WebviewUrl::App("index.html".into()))
                    .title("Floppy Station")
                    .inner_size(330.0, 118.0)
                    .resizable(false)
                    .maximizable(false)
                    .minimizable(false)
                    .decorations(false)
                    .transparent(true)
                    .shadow(false)
                    .always_on_top(true)
                    .skip_taskbar(true)
                    .focused(false)
                    .focusable(false)
                    .center()
                    .build();
            let loading = app.state::<LoadingBox>();
            match result {
                // Closed while it was being built.
                Ok(window) if !loading.is_open() => {
                    let _ = window.destroy();
                }
                Ok(_) => {}
                Err(error) => {
                    log::error!("could not open the loading box: {error}");
                    loading.lock().window = false;
                }
            }
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BoxInner> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }
}
