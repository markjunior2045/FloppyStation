//! Local preferences, stored in `%APPDATA%\floppy-station\settings.toml`.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

use crate::floppy::{DriveLetter, PollIntervals};

pub const CURRENT_VERSION: u32 = 1;
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 1000;
pub const DEFAULT_EMPTY_POLL_INTERVAL_MS: u64 = 3000;
pub const MIN_POLL_INTERVAL_MS: u64 = 250;
pub const MAX_POLL_INTERVAL_MS: u64 = 10_000;

const APP_DIR: &str = "floppy-station";
const FILE_NAME: &str = "settings.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Format version, for future migrations.
    pub version: u32,
    /// Drive to watch.
    pub drive: DriveLetter,
    /// Time between two checks of the drive while it has a disk (or is
    /// missing), in milliseconds.
    pub poll_interval_ms: u64,
    /// Time between two checks of an empty drive, in milliseconds. Longer
    /// than `poll_interval_ms` because checking an empty floppy drive makes
    /// it click.
    pub empty_poll_interval_ms: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            drive: DriveLetter::default(),
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            empty_poll_interval_ms: DEFAULT_EMPTY_POLL_INTERVAL_MS,
        }
    }
}

#[derive(Debug)]
pub enum SettingsError {
    /// `%APPDATA%` could not be resolved.
    AppDataUnavailable,
    Io(io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl SettingsError {
    /// Stable code for the error, to be translated by the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Self::AppDataUnavailable => "settings_appdata_unavailable",
            Self::Io(_) => "settings_io",
            Self::Parse(_) => "settings_invalid",
            Self::Serialize(_) => "settings_serialize",
        }
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AppDataUnavailable => write!(f, "could not resolve the AppData folder"),
            Self::Io(error) => write!(f, "{error}"),
            Self::Parse(error) => write!(f, "{error}"),
            Self::Serialize(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SettingsError {}

impl Settings {
    /// Default location of the settings file.
    pub fn default_path() -> Result<PathBuf, SettingsError> {
        Ok(roaming_app_data()?.join(APP_DIR).join(FILE_NAME))
    }

    /// Loads the settings from the default location. Never fails: a missing
    /// file gives the defaults, and an unreadable or invalid one gives the
    /// defaults plus the error, so the caller can report it.
    pub fn load_or_default() -> (Self, Option<SettingsError>) {
        match Self::default_path().and_then(|path| Self::load_from(&path)) {
            Ok(settings) => (settings, None),
            Err(error) => (Self::default(), Some(error)),
        }
    }

    /// Loads the settings from `path`. A missing file gives the defaults.
    pub fn load_from(path: &Path) -> Result<Self, SettingsError> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(SettingsError::Io(error)),
        }
    }

    /// Parses settings text. Missing fields take their default value, and
    /// out-of-range values are clamped.
    pub fn parse(text: &str) -> Result<Self, SettingsError> {
        let mut settings: Self = toml::from_str(text).map_err(SettingsError::Parse)?;
        settings.normalize();
        Ok(settings)
    }

    /// Writes the settings to `path`, creating its folder if needed. Writes
    /// to a temporary file first, so a failure never leaves a broken file.
    pub fn save_to(&self, path: &Path) -> Result<(), SettingsError> {
        let text = toml::to_string(self).map_err(SettingsError::Serialize)?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(SettingsError::Io)?;
        }
        let temp = path.with_extension("toml.tmp");
        fs::write(&temp, text).map_err(SettingsError::Io)?;
        fs::rename(&temp, path).map_err(SettingsError::Io)
    }

    pub fn poll_interval(&self) -> Duration {
        Duration::from_millis(self.poll_interval_ms)
    }

    pub fn poll_intervals(&self) -> PollIntervals {
        PollIntervals {
            present: Duration::from_millis(self.poll_interval_ms),
            empty: Duration::from_millis(self.empty_poll_interval_ms),
        }
    }

    fn normalize(&mut self) {
        clamp_interval("poll_interval_ms", &mut self.poll_interval_ms);
        clamp_interval("empty_poll_interval_ms", &mut self.empty_poll_interval_ms);
    }
}

fn clamp_interval(name: &str, value: &mut u64) {
    let clamped = (*value).clamp(MIN_POLL_INTERVAL_MS, MAX_POLL_INTERVAL_MS);
    if clamped != *value {
        log::warn!("{name} = {value} is out of range, using {clamped}");
        *value = clamped;
    }
}

fn roaming_app_data() -> Result<PathBuf, SettingsError> {
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, None)
            .map_err(|_| SettingsError::AppDataUnavailable)?;
        let result = path.to_string();
        CoTaskMemFree(Some(path.0 as *const _));
        result
            .map(PathBuf::from)
            .map_err(|_| SettingsError::AppDataUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(Settings::parse("").unwrap(), Settings::default());
    }

    #[test]
    fn parses_all_fields() {
        let settings = Settings::parse(
            r#"
            version = 1
            drive = "b"
            poll_interval_ms = 2000
            empty_poll_interval_ms = 5000
            "#,
        )
        .unwrap();
        assert_eq!(settings.drive, DriveLetter::parse("B").unwrap());
        assert_eq!(
            settings.poll_intervals(),
            PollIntervals {
                present: Duration::from_secs(2),
                empty: Duration::from_secs(5),
            }
        );
    }

    #[test]
    fn clamps_poll_interval() {
        let fast = Settings::parse("poll_interval_ms = 10").unwrap();
        assert_eq!(fast.poll_interval_ms, MIN_POLL_INTERVAL_MS);
        let slow = Settings::parse("poll_interval_ms = 999999").unwrap();
        assert_eq!(slow.poll_interval_ms, MAX_POLL_INTERVAL_MS);
        let empty = Settings::parse("empty_poll_interval_ms = 0").unwrap();
        assert_eq!(empty.empty_poll_interval_ms, MIN_POLL_INTERVAL_MS);
    }

    #[test]
    fn rejects_invalid_drive() {
        let error = Settings::parse(r#"drive = "AB:""#).unwrap_err();
        assert_eq!(error.code(), "settings_invalid");
    }

    #[test]
    fn rejects_invalid_toml() {
        assert!(Settings::parse("drive = ").is_err());
    }

    #[test]
    fn missing_file_gives_defaults() {
        let path = std::env::temp_dir().join("floppy-station-test-missing/settings.toml");
        assert_eq!(Settings::load_from(&path).unwrap(), Settings::default());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("floppy-station-test-{}", std::process::id()));
        let path = dir.join("settings.toml");
        let settings = Settings {
            drive: DriveLetter::parse("C").unwrap(),
            poll_interval_ms: 1500,
            ..Settings::default()
        };
        settings.save_to(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#"drive = "C:""#), "{text}");
        assert_eq!(Settings::load_from(&path).unwrap(), settings);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolves_default_path() {
        let path = Settings::default_path().unwrap();
        assert!(
            path.ends_with(r"floppy-station\settings.toml"),
            "{}",
            path.display()
        );
    }
}
