//! What the UI receives: every drive and game event as a code (the key in
//! the language files) plus its data. Kept here so the core library stays
//! free of UI concerns.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use floppy_station_core::controller::GameEvent;
use floppy_station_core::diskconfig::DiskContent;
use floppy_station_core::floppy::FloppyEvent;
use floppy_station_core::kinds::GameTarget;
use floppy_station_core::launcher::{CloseMode, CloseOutcome, LaunchError};
use floppy_station_core::settings::SettingsError;
use serde::Serialize;

/// An event for the UI. `seq` is set when the event is recorded.
#[derive(Debug, Clone, Serialize)]
pub struct AppEvent {
    pub seq: u64,
    /// Milliseconds since 1970.
    pub at: u64,
    pub code: &'static str,
    pub params: BTreeMap<&'static str, String>,
}

impl AppEvent {
    fn new(code: &'static str) -> Self {
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        Self {
            seq: 0,
            at,
            code,
            params: BTreeMap::new(),
        }
    }

    fn with(mut self, key: &'static str, value: impl ToString) -> Self {
        self.params.insert(key, value.to_string());
        self
    }
}

pub fn from_floppy(event: &FloppyEvent) -> AppEvent {
    AppEvent::new(event.code()).with("drive", event.drive())
}

pub fn from_game(event: &GameEvent) -> AppEvent {
    match event {
        GameEvent::DiskRead { drive, inspection } => {
            let event = from_content(&inspection.content).with("drive", drive);
            match inspection.write_protected {
                Some(protected) => event.with("writeProtected", protected),
                None => event,
            }
        }
        GameEvent::StartupDiskIgnored { drive, name } => {
            game("game_ignored_at_startup", drive, name)
        }
        GameEvent::Untrusted { drive, name, hash } => {
            game("launch_untrusted", drive, name).with("hash", hash)
        }
        GameEvent::Launched { drive, name } => game("game_launched", drive, name),
        GameEvent::Reattached { drive, name } => game("game_reattached", drive, name),
        GameEvent::LaunchFailed { drive, name, error } => {
            let event = game(error.code(), drive, name);
            match error {
                LaunchError::Failed(code) => event.with("error", code),
                _ => event,
            }
        }
        GameEvent::LeftOpen {
            drive,
            name,
            reason,
        } => game(reason.code(), drive, name),
        GameEvent::Closing { drive, name, mode } => {
            let event = game("game_closing", drive, name);
            match mode {
                CloseMode::Graceful(timeout) => event
                    .with("mode", "close")
                    .with("timeout", timeout.as_secs()),
                CloseMode::Force => event.with("mode", "force"),
            }
        }
        GameEvent::Closed {
            drive,
            name,
            outcome,
        } => {
            let event = game(outcome.code(), drive, name);
            match outcome {
                CloseOutcome::Failed { remaining } => event.with("remaining", remaining),
                _ => event,
            }
        }
    }
}

pub fn from_settings_error(error: &SettingsError) -> AppEvent {
    AppEvent::new(error.code()).with("detail", error)
}

fn game(code: &'static str, drive: impl ToString, name: &str) -> AppEvent {
    AppEvent::new(code).with("drive", drive).with("name", name)
}

fn from_content(content: &DiskContent) -> AppEvent {
    let event = AppEvent::new(content.code());
    match content {
        DiskContent::Configured { config, hash } => {
            let target = match &config.target {
                GameTarget::Steam(game) => game.app_id.to_string(),
                GameTarget::Exe(game) => game.path.display().to_string(),
            };
            let event = event
                .with("name", &config.name)
                .with("kind", config.target.kind())
                .with("target", target)
                .with("hash", hash);
            match &config.process {
                Some(process) => event.with("process", process),
                None => event,
            }
        }
        DiskContent::Invalid(error) => event.with("reason", error.code()).with("detail", error),
        DiskContent::UnsupportedVersion(version) => event.with("version", version),
        DiskContent::UnsupportedKind(kind) => event.with("kind", kind),
        DiskContent::Unreadable(Some(code)) => event.with("error", code),
        _ => event,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use floppy_station_core::controller::LeftOpenReason;
    use floppy_station_core::diskconfig::{
        ConfigError, ConfigHash, DiskConfig, DiskInspection, WriteError,
    };
    use floppy_station_core::floppy::{DriveLetter, InsertOrigin, MediaInfo, RemovalCause};
    use floppy_station_core::settings::Settings;

    use super::*;

    const LOCALE: &str = include_str!("../../src/locales/en.json");

    fn steam_config() -> DiskConfig {
        DiskConfig::parse(
            b"version = 1\n[game]\nname = \"Game\"\nkind = \"steam\"\nsteam_app_id = 1\n",
        )
        .unwrap()
    }

    /// One event of every kind the UI can receive.
    fn all_events() -> Vec<AppEvent> {
        let drive = DriveLetter::parse("A").unwrap();
        let name = || "Game".to_owned();
        let mut floppy = [
            FloppyEvent::DriveConnected { drive },
            FloppyEvent::DriveDisconnected { drive },
            FloppyEvent::MediaInserted {
                drive,
                origin: InsertOrigin::Inserted,
            },
            FloppyEvent::MediaRemoved {
                drive,
                cause: RemovalCause::Ejected,
            },
        ]
        .iter()
        .map(from_floppy)
        .collect::<Vec<_>>();

        let contents = [
            DiskContent::Configured {
                config: steam_config(),
                hash: ConfigHash::of(b""),
            },
            DiskContent::NotConfigured,
            DiskContent::Invalid(ConfigError::TooLarge),
            DiskContent::UnsupportedVersion(2),
            DiskContent::UnsupportedKind("epic".into()),
            DiskContent::Unformatted,
            DiskContent::NoDisk,
            DiskContent::Unreadable(Some(5)),
        ];
        let mut games: Vec<GameEvent> = contents
            .into_iter()
            .map(|content| GameEvent::DiskRead {
                drive,
                inspection: DiskInspection {
                    content,
                    write_protected: Some(false),
                    volume: MediaInfo::default(),
                },
            })
            .collect();
        games.extend([
            GameEvent::StartupDiskIgnored {
                drive,
                name: name(),
            },
            GameEvent::Untrusted {
                drive,
                name: name(),
                hash: ConfigHash::of(b""),
            },
            GameEvent::Launched {
                drive,
                name: name(),
            },
            GameEvent::Reattached {
                drive,
                name: name(),
            },
            GameEvent::Closing {
                drive,
                name: name(),
                mode: CloseMode::Graceful(Duration::from_secs(10)),
            },
        ]);
        for error in [
            LaunchError::NotFound,
            LaunchError::ElevationRequired,
            LaunchError::SteamMissing,
            LaunchError::Failed(5),
        ] {
            games.push(GameEvent::LaunchFailed {
                drive,
                name: name(),
                error,
            });
        }
        for reason in [
            LeftOpenReason::Setting,
            LeftOpenReason::DriveDisconnected,
            LeftOpenReason::NothingToClose,
        ] {
            games.push(GameEvent::LeftOpen {
                drive,
                name: name(),
                reason,
            });
        }
        for outcome in [
            CloseOutcome::AlreadyClosed,
            CloseOutcome::Closed,
            CloseOutcome::Forced,
            CloseOutcome::Failed { remaining: 1 },
        ] {
            games.push(GameEvent::Closed {
                drive,
                name: name(),
                outcome,
            });
        }
        floppy.extend(games.iter().map(from_game));
        floppy.push(from_settings_error(&SettingsError::AppDataUnavailable));
        floppy.push(from_settings_error(
            &Settings::parse("drive = ").unwrap_err(),
        ));
        floppy
    }

    #[test]
    fn every_event_has_an_english_text() {
        let locale: BTreeMap<String, String> = serde_json::from_str(LOCALE).unwrap();
        for event in all_events() {
            let text = locale
                .get(event.code)
                .unwrap_or_else(|| panic!("en.json has no text for {}", event.code));
            // Every placeholder in the text must come with the event.
            for placeholder in text.split('{').skip(1) {
                let key = placeholder.split('}').next().unwrap();
                assert!(
                    event.params.contains_key(key),
                    "{} uses {{{key}}}, which the event lacks",
                    event.code
                );
            }
        }
    }

    #[test]
    fn every_write_error_has_an_english_text() {
        let locale: BTreeMap<String, String> = serde_json::from_str(LOCALE).unwrap();
        let errors = [
            WriteError::WriteProtected,
            WriteError::InUse,
            WriteError::DiskFull,
            WriteError::Removed,
            WriteError::VerifyFailed,
            WriteError::Io(std::io::Error::other("test")),
        ];
        for error in errors {
            assert!(locale.contains_key(error.code()), "{}", error.code());
        }
    }
}
