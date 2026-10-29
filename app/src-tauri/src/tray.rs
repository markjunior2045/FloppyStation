//! The tray icon: always there while the app runs, with the state in its
//! tooltip and menu.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::locale::t;
use crate::watcher::Watcher;
use crate::windows;

pub struct Tray {
    icon: TrayIcon,
    state: MenuItem<Wry>,
    open: MenuItem<Wry>,
    detect: CheckMenuItem<Wry>,
    exit: MenuItem<Wry>,
}

impl Tray {
    pub fn create(app: &AppHandle, line: &str, detection: bool) -> tauri::Result<Self> {
        let state = MenuItem::with_id(app, "state", line, false, None::<&str>)?;
        let open = MenuItem::with_id(app, "open", t("tray.open", &[]), true, None::<&str>)?;
        let detect = CheckMenuItem::with_id(
            app,
            "detect",
            t("tray.detect", &[]),
            true,
            detection,
            None::<&str>,
        )?;
        let exit = MenuItem::with_id(app, "exit", t("tray.exit", &[]), true, None::<&str>)?;
        let menu = Menu::with_items(
            app,
            &[
                &state,
                &PredefinedMenuItem::separator(app)?,
                &open,
                &detect,
                &PredefinedMenuItem::separator(app)?,
                &exit,
            ],
        )?;

        let mut builder = TrayIconBuilder::with_id("main")
            .tooltip(tooltip(line))
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| match event.id().as_ref() {
                "open" => windows::open_main(app),
                "detect" => {
                    let watcher = app.state::<Watcher>();
                    watcher.set_detection(!watcher.detection());
                }
                "exit" => app.exit(0),
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    windows::open_main(tray.app_handle());
                }
            });
        if let Some(icon) = app.default_window_icon() {
            builder = builder.icon(icon.clone());
        }
        let icon = builder.build(app)?;
        Ok(Self {
            icon,
            state,
            open,
            detect,
            exit,
        })
    }

    /// Shows the new state. The fixed items are set again too, in case the
    /// language changed.
    pub fn update(&self, line: &str, detection: bool) {
        let _ = self.state.set_text(line);
        let _ = self.icon.set_tooltip(Some(tooltip(line)));
        let _ = self.detect.set_checked(detection);
        let _ = self.open.set_text(t("tray.open", &[]));
        let _ = self.detect.set_text(t("tray.detect", &[]));
        let _ = self.exit.set_text(t("tray.exit", &[]));
    }
}

fn tooltip(line: &str) -> String {
    t("tray.tooltip", &[("state", line)])
}
