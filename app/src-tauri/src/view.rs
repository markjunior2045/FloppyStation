//! The current state of the drive, kept up to date from the events: sent to
//! the window, and summed up in one line for the tray.

use std::collections::BTreeMap;

use floppy_station_core::controller::GameEvent;
use floppy_station_core::diskconfig::{ConfigHash, DiskConfig, DiskContent};
use floppy_station_core::floppy::{DriveLetter, FloppyEvent};
use floppy_station_core::kinds::{GameTarget, exe};
use floppy_station_core::trust;
use serde::Serialize;

use crate::events;
use crate::locale::t;

/// Everything the window shows about the drive.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub drive: String,
    pub detection: bool,
    /// `off`, `starting`, `missing`, `empty`, `reading` or `present`.
    pub drive_status: &'static str,
    /// The state in one line, as in the tray.
    pub line: String,
    /// What the disk in the drive contains, once read.
    pub disk: Option<DiskView>,
    /// The last thing that happened to a game.
    pub game: Option<GameView>,
    /// The game launched from the disk in the drive, if any.
    pub launched: Option<String>,
    /// Problem loading the settings at startup, as a code.
    pub settings_error: Option<&'static str>,
    /// No settings saved yet: the window shows the welcome.
    pub first_run: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskView {
    /// A `disk_*` code.
    pub code: &'static str,
    pub params: BTreeMap<&'static str, String>,
    pub write_protected: Option<bool>,
    pub config: Option<ConfigView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    pub name: String,
    pub kind: &'static str,
    pub app_id: Option<u32>,
    pub path: Option<String>,
    pub args: Vec<String>,
    /// `args` as one line, as typed in the form.
    pub args_line: String,
    pub process: Option<String>,
    pub hash: String,
    /// Whether a program (`kind = "exe"`) is approved on this PC; always
    /// true for Steam games, which need no approval.
    pub approved: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameView {
    pub code: &'static str,
    pub params: BTreeMap<&'static str, String>,
}

#[derive(Debug)]
pub struct DriveView {
    drive: DriveLetter,
    detection: bool,
    /// `None` until the drive is first checked.
    connected: Option<bool>,
    reading: bool,
    disk: Option<DiskView>,
    /// The configuration of the disk in the drive, for approving it.
    config: Option<(DiskConfig, ConfigHash)>,
    game: Option<GameView>,
    launched: Option<String>,
}

impl DriveView {
    pub fn new(drive: DriveLetter, detection: bool) -> Self {
        Self {
            drive,
            detection,
            connected: None,
            reading: false,
            disk: None,
            config: None,
            game: None,
            launched: None,
        }
    }

    /// Whether a disk is in and read, so it can be written to.
    pub fn has_disk(&self) -> bool {
        self.drive_status() == "present"
    }

    /// The configuration of the disk in the drive, if it has a valid one.
    pub fn config(&self) -> Option<&(DiskConfig, ConfigHash)> {
        self.config.as_ref()
    }

    /// The disk's new content after the app wrote or removed its
    /// configuration.
    pub fn set_content(&mut self, content: &DiskContent) {
        let write_protected = self.disk.as_ref().and_then(|disk| disk.write_protected);
        self.set_disk(content, write_protected);
    }

    /// Checks again whether the disk's program is approved, after an
    /// approval was added or revoked.
    pub fn refresh_approval(&mut self) {
        if let (Some(view), Some((config, hash))) = (
            self.disk.as_mut().and_then(|disk| disk.config.as_mut()),
            &self.config,
        ) {
            view.approved = is_approved(config, hash);
        }
    }

    fn set_disk(&mut self, content: &DiskContent, write_protected: Option<bool>) {
        self.config = match content {
            DiskContent::Configured { config, hash } => Some((config.clone(), *hash)),
            _ => None,
        };
        self.disk = match content {
            DiskContent::NoDisk => None,
            content => Some(disk_view(content, write_protected)),
        };
    }

    /// Starts over, for another drive or after detection was turned on or
    /// off. The last game message stays.
    pub fn reset(&mut self, drive: DriveLetter, detection: bool) {
        let game = self.game.take();
        *self = Self::new(drive, detection);
        self.game = game;
    }

    pub fn on_floppy(&mut self, event: &FloppyEvent) {
        match event {
            FloppyEvent::DriveConnected { .. } => self.connected = Some(true),
            FloppyEvent::DriveDisconnected { .. } => {
                self.connected = Some(false);
                self.clear_disk();
            }
            FloppyEvent::MediaInserted { .. } => {
                self.clear_disk();
                self.reading = true;
            }
            FloppyEvent::MediaRemoved { .. } => self.clear_disk(),
        }
    }

    pub fn on_game(&mut self, event: &GameEvent) {
        match event {
            GameEvent::DiskRead { inspection, .. } => {
                self.reading = false;
                self.set_disk(&inspection.content, inspection.write_protected);
                return;
            }
            GameEvent::Untrusted { .. } => {
                if let Some(config) = self.disk.as_mut().and_then(|disk| disk.config.as_mut()) {
                    config.approved = false;
                }
            }
            GameEvent::Launched { name, .. } | GameEvent::Reattached { name, .. } => {
                self.launched = Some(name.clone());
            }
            GameEvent::LeftOpen { .. } | GameEvent::Closing { .. } => self.launched = None,
            _ => {}
        }
        let event = events::from_game(event);
        self.game = Some(GameView {
            code: event.code,
            params: event.params,
        });
    }

    pub fn state(&self, settings_error: Option<&'static str>) -> AppState {
        AppState {
            drive: self.drive.to_string(),
            detection: self.detection,
            drive_status: self.drive_status(),
            line: self.line(),
            disk: self.disk.clone(),
            game: self.game.clone(),
            launched: self.launched.clone(),
            settings_error,
            first_run: false,
        }
    }

    /// The state in one line, e.g. `A: Counter-Strike 2 (launched)`.
    pub fn line(&self) -> String {
        let drive = self.drive.to_string();
        let with_drive = |key| t(key, &[("drive", &drive)]);
        match self.drive_status() {
            "off" => t("tray.detection_off", &[]),
            "starting" => t("tray.starting", &[]),
            "missing" => with_drive("tray.drive_missing"),
            "empty" => with_drive("tray.no_disk"),
            "reading" => with_drive("tray.reading"),
            _ => match (&self.launched, &self.disk) {
                (Some(name), _) => t("tray.launched", &[("drive", &drive), ("name", name)]),
                (None, Some(disk)) => match (&disk.config, disk.code) {
                    (Some(config), _) => t(
                        "tray.configured",
                        &[("drive", &drive), ("name", &config.name)],
                    ),
                    (None, "disk_not_configured") => with_drive("tray.not_configured"),
                    (None, _) => with_drive("tray.unusable"),
                },
                (None, None) => with_drive("tray.no_disk"),
            },
        }
    }

    fn drive_status(&self) -> &'static str {
        if !self.detection {
            return "off";
        }
        match self.connected {
            None => "starting",
            Some(false) => "missing",
            Some(true) if self.reading => "reading",
            Some(true) if self.disk.is_some() => "present",
            Some(true) => "empty",
        }
    }

    fn clear_disk(&mut self) {
        self.reading = false;
        self.disk = None;
        self.config = None;
        self.launched = None;
    }
}

/// Programs need approval; Steam games don't.
fn is_approved(config: &DiskConfig, hash: &ConfigHash) -> bool {
    match config.target {
        GameTarget::Exe(_) => trust::is_approved(hash),
        _ => true,
    }
}

fn disk_view(content: &DiskContent, write_protected: Option<bool>) -> DiskView {
    let event = events::from_content(content);
    let config = match content {
        DiskContent::Configured { config, hash } => {
            let (app_id, path, args) = match &config.target {
                GameTarget::Steam(game) => (Some(game.app_id), None, Vec::new()),
                GameTarget::Exe(game) => (
                    None,
                    Some(game.path.display().to_string()),
                    game.args.clone(),
                ),
            };
            let approved = is_approved(config, hash);
            Some(ConfigView {
                args_line: exe::join_args(&args),
                name: config.name.clone(),
                kind: config.target.kind(),
                app_id,
                path,
                args,
                process: config.process.clone(),
                hash: hash.to_string(),
                approved,
            })
        }
        _ => None,
    };
    DiskView {
        code: event.code,
        params: event.params,
        write_protected,
        config,
    }
}

#[cfg(test)]
mod tests {
    use floppy_station_core::diskconfig::{ConfigHash, DiskConfig, DiskInspection};
    use floppy_station_core::floppy::{InsertOrigin, MediaInfo, RemovalCause};

    use super::*;

    fn a() -> DriveLetter {
        DriveLetter::parse("A").unwrap()
    }

    fn read(content: DiskContent) -> GameEvent {
        GameEvent::DiskRead {
            drive: a(),
            inspection: DiskInspection {
                content,
                write_protected: Some(false),
                volume: MediaInfo::default(),
            },
        }
    }

    #[test]
    fn follows_a_disk_from_insertion_to_removal() {
        let mut view = DriveView::new(a(), true);
        assert_eq!(view.line(), "Starting...");
        view.on_floppy(&FloppyEvent::DriveConnected { drive: a() });
        assert_eq!(view.line(), "A: no disk");
        view.on_floppy(&FloppyEvent::MediaInserted {
            drive: a(),
            origin: InsertOrigin::Inserted,
        });
        assert_eq!(view.line(), "A: reading disk...");
        let text = b"version = 1\n[game]\nname = \"Doom\"\nkind = \"steam\"\nsteam_app_id = 1\n";
        view.on_game(&read(DiskContent::Configured {
            config: DiskConfig::parse(text).unwrap(),
            hash: ConfigHash::of(text),
        }));
        assert_eq!(view.line(), "A: Doom");
        let state = view.state(None);
        assert_eq!(state.drive_status, "present");
        let config = state.disk.unwrap().config.unwrap();
        assert_eq!(
            (config.kind, config.app_id, config.approved),
            ("steam", Some(1), true)
        );

        view.on_game(&GameEvent::Launched {
            drive: a(),
            name: "Doom".into(),
        });
        assert_eq!(view.line(), "A: Doom (launched)");
        assert_eq!(view.state(None).game.unwrap().code, "game_launched");
        view.on_floppy(&FloppyEvent::MediaRemoved {
            drive: a(),
            cause: RemovalCause::Ejected,
        });
        assert_eq!(view.line(), "A: no disk");
        assert_eq!(view.state(None).launched, None);
    }

    #[test]
    fn shows_problems_and_missing_drive() {
        let mut view = DriveView::new(a(), true);
        view.on_floppy(&FloppyEvent::DriveConnected { drive: a() });
        view.on_game(&read(DiskContent::NotConfigured));
        assert_eq!(view.line(), "A: disk not configured");
        view.on_game(&read(DiskContent::Unformatted));
        assert_eq!(view.line(), "A: disk can't be used");
        view.on_floppy(&FloppyEvent::DriveDisconnected { drive: a() });
        assert_eq!(view.line(), "Drive A: not found");
        view.reset(a(), false);
        assert_eq!(view.line(), "Detection is off");
        assert_eq!(view.state(None).drive_status, "off");
    }
}
