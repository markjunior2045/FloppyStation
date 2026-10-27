//! Disk configurations the user approved, stored in
//! `%APPDATA%\floppy-station\trusted.toml`.
//!
//! A `kind = "exe"` disk can run any program on the PC, so it only runs if
//! the hash of its configuration file is in this list. Any change to the
//! file changes the hash, and the disk needs approval again. Steam disks
//! only open games by AppID and need no approval.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::diskconfig::{ConfigHash, DiskConfig};
use crate::kinds::GameTarget;
use crate::settings::{SettingsError, app_dir};

pub const CURRENT_VERSION: u32 = 1;
const FILE_NAME: &str = "trusted.toml";

/// An approved configuration. Only `hash` matters; the rest is there so the
/// user can tell the entries apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustEntry {
    /// SHA-256 of the configuration file, in hex.
    pub hash: String,
    pub name: String,
    /// The program the configuration runs.
    pub path: String,
    /// When it was approved, in local time.
    pub added: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrustStore {
    version: u32,
    #[serde(rename = "config")]
    entries: Vec<TrustEntry>,
}

#[derive(Debug)]
pub enum TrustError {
    AppDataUnavailable,
    Io(io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl TrustError {
    /// Stable code for the error, to be translated by the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Self::AppDataUnavailable => "trust_appdata_unavailable",
            Self::Io(_) => "trust_io",
            Self::Parse(_) => "trust_invalid",
            Self::Serialize(_) => "trust_serialize",
        }
    }
}

impl fmt::Display for TrustError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AppDataUnavailable => write!(f, "could not resolve the AppData folder"),
            Self::Io(error) => write!(f, "{error}"),
            Self::Parse(error) => write!(f, "{error}"),
            Self::Serialize(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for TrustError {}

/// Why an entry could not be revoked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevokeError {
    NotFound,
    /// The hash prefix matches several entries.
    Ambiguous(usize),
}

impl TrustStore {
    pub fn default_path() -> Result<PathBuf, TrustError> {
        app_dir()
            .map(|dir| dir.join(FILE_NAME))
            .map_err(|error| match error {
                SettingsError::Io(error) => TrustError::Io(error),
                _ => TrustError::AppDataUnavailable,
            })
    }

    /// Loads the list from `path`. A missing file gives an empty list.
    pub fn load_from(path: &Path) -> Result<Self, TrustError> {
        match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(TrustError::Parse),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(TrustError::Io(error)),
        }
    }

    pub fn load() -> Result<Self, TrustError> {
        Self::load_from(&Self::default_path()?)
    }

    /// Writes the list to `path` through a temporary file.
    pub fn save_to(&self, path: &Path) -> Result<(), TrustError> {
        let file = Self {
            version: CURRENT_VERSION,
            entries: self.entries.clone(),
        };
        let text = toml::to_string(&file).map_err(TrustError::Serialize)?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(TrustError::Io)?;
        }
        let temp = path.with_extension("toml.tmp");
        fs::write(&temp, text).map_err(TrustError::Io)?;
        fs::rename(&temp, path).map_err(TrustError::Io)
    }

    pub fn save(&self) -> Result<(), TrustError> {
        self.save_to(&Self::default_path()?)
    }

    pub fn entries(&self) -> &[TrustEntry] {
        &self.entries
    }

    pub fn is_trusted(&self, hash: &ConfigHash) -> bool {
        let hash = hash.to_string();
        self.entries.iter().any(|entry| entry.hash == hash)
    }

    /// Approves a configuration. Returns `false` if it already was.
    pub fn add(&mut self, hash: &ConfigHash, config: &DiskConfig) -> bool {
        if self.is_trusted(hash) {
            return false;
        }
        let path = match &config.target {
            GameTarget::Exe(game) => game.path.display().to_string(),
            GameTarget::Steam(game) => format!("steam://run/{}", game.app_id),
        };
        self.entries.push(TrustEntry {
            hash: hash.to_string(),
            name: config.name.clone(),
            path,
            added: local_time(),
        });
        true
    }

    /// Removes the entry whose hash starts with `prefix`.
    pub fn revoke(&mut self, prefix: &str) -> Result<TrustEntry, RevokeError> {
        let prefix = prefix.trim().to_ascii_lowercase();
        let matches: Vec<usize> = (0..self.entries.len())
            .filter(|&i| self.entries[i].hash.starts_with(&prefix))
            .collect();
        match matches[..] {
            [] => Err(RevokeError::NotFound),
            [i] => Ok(self.entries.remove(i)),
            _ => Err(RevokeError::Ambiguous(matches.len())),
        }
    }
}

/// Loads the list, approves a configuration and saves it. Returns `false` if
/// it already was approved.
pub fn approve(hash: &ConfigHash, config: &DiskConfig) -> Result<bool, TrustError> {
    let mut store = TrustStore::load()?;
    if !store.add(hash, config) {
        return Ok(false);
    }
    store.save()?;
    Ok(true)
}

/// Whether a configuration is approved, reading the list again so approvals
/// made by another instance (such as the `trust` command) count right away.
/// An unreadable list approves nothing.
pub fn is_approved(hash: &ConfigHash) -> bool {
    match TrustStore::load() {
        Ok(store) => store.is_trusted(hash),
        Err(error) => {
            log::warn!(
                "could not read the approved configs ({}): {error}",
                error.code()
            );
            false
        }
    }
}

fn local_time() -> String {
    let t = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = r#"
version = 1

[game]
name = "Doom"
kind = "exe"
path = 'C:\Games\Doom\doom.exe'
"#;

    fn sample(text: &str) -> (ConfigHash, DiskConfig) {
        (
            ConfigHash::of(text.as_bytes()),
            DiskConfig::parse(text.as_bytes()).unwrap(),
        )
    }

    #[test]
    fn approves_exact_configs() {
        let (hash, config) = sample(EXE);
        let mut store = TrustStore::default();
        assert!(!store.is_trusted(&hash));
        assert!(store.add(&hash, &config));
        assert!(!store.add(&hash, &config));
        assert!(store.is_trusted(&hash));
        assert_eq!(store.entries()[0].path, r"C:\Games\Doom\doom.exe");

        let (edited, _) = sample(&EXE.replace("Doom\"", "Doom 2\""));
        assert!(!store.is_trusted(&edited));
    }

    #[test]
    fn revokes_by_prefix() {
        let mut store = TrustStore::default();
        let (a, config) = sample(EXE);
        let (b, _) = sample(&format!("{EXE}\n"));
        store.add(&a, &config);
        store.add(&b, &config);
        assert_eq!(store.revoke("zz"), Err(RevokeError::NotFound));
        assert_eq!(store.revoke(""), Err(RevokeError::Ambiguous(2)));
        let hex = a.to_string();
        assert_eq!(store.revoke(&hex[..12].to_uppercase()).unwrap().hash, hex);
        assert!(!store.is_trusted(&a));
        assert!(store.is_trusted(&b));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("floppy-station-trust-{}", std::process::id()));
        let path = dir.join(FILE_NAME);
        assert_eq!(TrustStore::load_from(&path).unwrap(), TrustStore::default());

        let (hash, config) = sample(EXE);
        let mut store = TrustStore::default();
        store.add(&hash, &config);
        store.save_to(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("[[config]]"), "{text}");
        assert!(TrustStore::load_from(&path).unwrap().is_trusted(&hash));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_invalid_file() {
        let error = toml::from_str::<TrustStore>("config = 3").unwrap_err();
        assert!(!error.to_string().is_empty());
    }
}
