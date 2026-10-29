//! Local preferences, stored in `%APPDATA%\floppy-station\settings.toml`.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};

use crate::floppy::{DriveLetter, PollIntervals};

pub const CURRENT_VERSION: u32 = 1;
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 1000;
pub const DEFAULT_EMPTY_POLL_INTERVAL_MS: u64 = 1000;
pub const MIN_POLL_INTERVAL_MS: u64 = 250;
pub const MAX_POLL_INTERVAL_MS: u64 = 10_000;
pub const DEFAULT_CLOSE_TIMEOUT_S: u64 = 10;
pub const MIN_CLOSE_TIMEOUT_S: u64 = 1;
pub const MAX_CLOSE_TIMEOUT_S: u64 = 60;

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
    /// Time between two checks of an empty drive, in milliseconds. Separate
    /// from `poll_interval_ms` because checking an empty floppy drive makes
    /// it click: longer intervals click less but notice disks later.
    pub empty_poll_interval_ms: u64,
    /// What to do with the game when its disk is removed.
    pub on_removal: RemovalAction,
    /// With [`RemovalAction::Close`], seconds to wait for the game to close
    /// before forcing it.
    pub close_timeout_s: u64,
    /// What to do with the game when the drive is unplugged.
    pub on_drive_disconnect: DisconnectAction,
    /// Whether the drive is watched at all. Off, the app ignores the drive:
    /// no checks (and no clicking), and disks neither launch nor close games.
    pub detection_enabled: bool,
    /// Whether the app shows a small box while it reads a disk.
    pub show_loading_box: bool,
    /// Quiet detection: an empty drive is waited on (one click every ~20 s,
    /// light on) instead of checked every `empty_poll_interval_ms`.
    pub quiet_detection: bool,
    /// Whether what needs the user (an unapproved program, a game that
    /// couldn't start or close) also becomes a Windows notification. Off by
    /// default.
    pub notifications: bool,
    /// Language of the app.
    pub language: Language,
}

/// The app's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Windows' display language, if the app has it, else English.
    #[default]
    Auto,
    En,
    Pt,
    Es,
}

impl Language {
    /// The language to show: `en`, `pt` or `es`, the names of the
    /// language files.
    pub fn resolve(self) -> &'static str {
        match self {
            Self::Auto => windows_language(),
            Self::En => "en",
            Self::Pt => "pt",
            Self::Es => "es",
        }
    }
}

/// The app's language closest to Windows' display language.
fn windows_language() -> &'static str {
    // The low 10 bits of a language ID are the language, whatever the
    // country (Brazil or Portugal, Spain or Mexico).
    match unsafe { GetUserDefaultUILanguage() } & 0x3ff {
        0x16 => "pt",
        0x0a => "es",
        _ => "en",
    }
}

/// What to do with a game when its disk is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemovalAction {
    /// Ask the game to close, and force it after `close_timeout_s`.
    #[default]
    Close,
    /// Force the game to close right away.
    Force,
    /// Leave the game open.
    #[serde(rename = "none")]
    Leave,
}

impl RemovalAction {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "close" => Some(Self::Close),
            "force" => Some(Self::Force),
            "none" => Some(Self::Leave),
            _ => None,
        }
    }
}

/// What to do with a game when the drive is unplugged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisconnectAction {
    /// Same as removing the disk.
    #[default]
    Removal,
    /// Leave the game open.
    Ignore,
}

impl DisconnectAction {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "removal" => Some(Self::Removal),
            "ignore" => Some(Self::Ignore),
            _ => None,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            drive: DriveLetter::default(),
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            empty_poll_interval_ms: DEFAULT_EMPTY_POLL_INTERVAL_MS,
            on_removal: RemovalAction::default(),
            close_timeout_s: DEFAULT_CLOSE_TIMEOUT_S,
            on_drive_disconnect: DisconnectAction::default(),
            detection_enabled: true,
            show_loading_box: true,
            quiet_detection: true,
            notifications: false,
            language: Language::Auto,
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
        Ok(app_dir()?.join(FILE_NAME))
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

    pub fn close_timeout(&self) -> Duration {
        Duration::from_secs(self.close_timeout_s)
    }

    /// Clamps out-of-range values, as when loading the file. For settings
    /// coming from elsewhere, such as the UI.
    pub fn normalize(&mut self) {
        let interval = MIN_POLL_INTERVAL_MS..=MAX_POLL_INTERVAL_MS;
        clamp("poll_interval_ms", &mut self.poll_interval_ms, &interval);
        clamp(
            "empty_poll_interval_ms",
            &mut self.empty_poll_interval_ms,
            &interval,
        );
        let timeout = MIN_CLOSE_TIMEOUT_S..=MAX_CLOSE_TIMEOUT_S;
        clamp("close_timeout_s", &mut self.close_timeout_s, &timeout);
    }
}

fn clamp(name: &str, value: &mut u64, range: &std::ops::RangeInclusive<u64>) {
    let clamped = (*value).clamp(*range.start(), *range.end());
    if clamped != *value {
        log::warn!("{name} = {value} is out of range, using {clamped}");
        *value = clamped;
    }
}

/// Folder of the app's local files, `%APPDATA%\floppy-station`.
pub(crate) fn app_dir() -> Result<PathBuf, SettingsError> {
    Ok(known_folder(&FOLDERID_RoamingAppData)
        .ok_or(SettingsError::AppDataUnavailable)?
        .join(APP_DIR))
}

/// Folder for what only matters on this PC (logs, helper copies),
/// `%LOCALAPPDATA%\floppy-station`.
pub fn local_dir() -> Option<PathBuf> {
    Some(known_folder(&FOLDERID_LocalAppData)?.join(APP_DIR))
}

fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let path = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let result = path.to_string();
        CoTaskMemFree(Some(path.0 as *const _));
        result.ok().map(PathBuf::from)
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
            on_removal = "none"
            close_timeout_s = 30
            on_drive_disconnect = "ignore"
            detection_enabled = false
            show_loading_box = false
            quiet_detection = false
            notifications = true
            language = "pt"
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
        assert_eq!(settings.on_removal, RemovalAction::Leave);
        assert_eq!(settings.close_timeout(), Duration::from_secs(30));
        assert_eq!(settings.on_drive_disconnect, DisconnectAction::Ignore);
        assert!(!settings.detection_enabled);
        assert!(!settings.show_loading_box);
        assert!(!settings.quiet_detection);
        assert!(settings.notifications);
        assert_eq!(settings.language, Language::Pt);
    }

    #[test]
    fn rejects_unknown_removal_action() {
        assert!(Settings::parse(r#"on_removal = "explode""#).is_err());
    }

    #[test]
    fn clamps_poll_interval() {
        let fast = Settings::parse("poll_interval_ms = 10").unwrap();
        assert_eq!(fast.poll_interval_ms, MIN_POLL_INTERVAL_MS);
        let slow = Settings::parse("poll_interval_ms = 999999").unwrap();
        assert_eq!(slow.poll_interval_ms, MAX_POLL_INTERVAL_MS);
        let empty = Settings::parse("empty_poll_interval_ms = 0").unwrap();
        assert_eq!(empty.empty_poll_interval_ms, MIN_POLL_INTERVAL_MS);
        let timeout = Settings::parse("close_timeout_s = 0").unwrap();
        assert_eq!(timeout.close_timeout_s, MIN_CLOSE_TIMEOUT_S);
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
            on_removal: RemovalAction::Leave,
            ..Settings::default()
        };
        settings.save_to(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#"drive = "C:""#), "{text}");
        assert!(text.contains(r#"on_removal = "none""#), "{text}");
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
