//! The configuration file on the disk (`floppy-station.toml`): reading,
//! validating, classifying the disk, writing and removing.
//!
//! Only ever touches its own two files ([`FILE_NAME`] and a temporary file
//! next to it); other files on the disk are never modified.

pub(crate) mod fields;
mod format;
mod media;

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};
use windows::Win32::Foundation::{
    ERROR_DISK_FULL, ERROR_FILE_NOT_FOUND, ERROR_HANDLE_DISK_FULL, ERROR_MEDIA_CHANGED,
    ERROR_NO_MEDIA_IN_DRIVE, ERROR_NOT_READY, ERROR_PATH_NOT_FOUND, ERROR_UNRECOGNIZED_VOLUME,
    ERROR_WRITE_PROTECT,
};

use crate::floppy::{DriveLetter, suppress_error_dialogs};
use crate::kinds::GameTarget;

pub use format::CURRENT_VERSION;

/// Name of the configuration file, in the root of the disk.
pub const FILE_NAME: &str = "floppy-station.toml";
const TEMP_FILE_NAME: &str = "floppy-station.toml.tmp";
/// Larger files are rejected: a real configuration is well under 1 KB.
pub const MAX_FILE_SIZE: usize = 64 * 1024;

/// A valid disk configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskConfig {
    /// Name shown to the user.
    pub name: String,
    /// What to launch.
    pub target: GameTarget,
    /// Name of the process to close when the disk is removed (e.g.
    /// `cs2.exe`), when it differs from what the app starts itself.
    pub process: Option<String>,
}

impl DiskConfig {
    /// Parses the bytes of a configuration file.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        format::parse(bytes)
    }

    /// The configuration as file text, in the current format version.
    pub fn to_toml(&self) -> String {
        format::to_toml(self)
    }
}

/// Why a configuration file could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Invalid(ConfigError),
    /// Made by a newer version of the app.
    UnsupportedVersion(i64),
    /// A `kind` this version does not know (possibly from a newer version).
    UnsupportedKind(String),
}

impl From<ConfigError> for ParseError {
    fn from(error: ConfigError) -> Self {
        Self::Invalid(error)
    }
}

/// What is wrong with an invalid configuration file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    TooLarge,
    NotUtf8,
    /// Not valid TOML. Holds the parser message, with line and column.
    Syntax(String),
    MissingField(&'static str),
    WrongType {
        field: &'static str,
        expected: &'static str,
    },
    InvalidValue {
        field: &'static str,
        reason: InvalidReason,
    },
}

impl ConfigError {
    /// Stable code for the error, to be translated by the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooLarge => "config_too_large",
            Self::NotUtf8 => "config_not_utf8",
            Self::Syntax(_) => "config_syntax",
            Self::MissingField(_) => "config_missing_field",
            Self::WrongType { .. } => "config_wrong_type",
            Self::InvalidValue { .. } => "config_invalid_value",
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "file is larger than {MAX_FILE_SIZE} bytes"),
            Self::NotUtf8 => write!(f, "file is not UTF-8 text"),
            Self::Syntax(message) => write!(f, "{}", message.trim_end()),
            Self::MissingField(field) => write!(f, "missing field {field:?}"),
            Self::WrongType { field, expected } => write!(f, "{field:?} must be {expected}"),
            Self::InvalidValue { field, reason } => write!(f, "{field:?}: {}", reason.code()),
        }
    }
}

/// Why a field value is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidReason {
    Empty,
    TooLong,
    NotPositive,
    OutOfRange,
    NotAbsolute,
    NotExe,
    HasDirectory,
}

impl InvalidReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::TooLong => "too_long",
            Self::NotPositive => "not_positive",
            Self::OutOfRange => "out_of_range",
            Self::NotAbsolute => "not_absolute_path",
            Self::NotExe => "not_exe",
            Self::HasDirectory => "has_directory",
        }
    }
}

/// SHA-256 of the exact bytes of a configuration file, used to remember
/// which configurations the user approved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConfigHash([u8; 32]);

impl ConfigHash {
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
}

impl fmt::Display for ConfigHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

/// What a disk contains, as far as Floppy Station is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskContent {
    Configured {
        config: DiskConfig,
        hash: ConfigHash,
    },
    NotConfigured,
    Invalid(ConfigError),
    UnsupportedVersion(i64),
    UnsupportedKind(String),
    /// Windows does not recognize the file system.
    Unformatted,
    /// There is no disk in the drive, or the drive itself is missing.
    NoDisk,
    /// The disk could not be read (damaged, or removed while reading). Holds
    /// the raw OS error code when known.
    Unreadable(Option<i32>),
}

impl DiskContent {
    /// Stable code for the content, to be translated by the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Configured { .. } => "disk_configured",
            Self::NotConfigured => "disk_not_configured",
            Self::Invalid(_) => "disk_config_invalid",
            Self::UnsupportedVersion(_) => "disk_unsupported_version",
            Self::UnsupportedKind(_) => "disk_unsupported_kind",
            Self::Unformatted => "disk_unformatted",
            Self::NoDisk => "disk_none",
            Self::Unreadable(_) => "disk_unreadable",
        }
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        match DiskConfig::parse(bytes) {
            Ok(config) => Self::Configured {
                config,
                hash: ConfigHash::of(bytes),
            },
            Err(ParseError::Invalid(error)) => Self::Invalid(error),
            Err(ParseError::UnsupportedVersion(version)) => Self::UnsupportedVersion(version),
            Err(ParseError::UnsupportedKind(kind)) => Self::UnsupportedKind(kind),
        }
    }

    fn from_read_error(error: &io::Error) -> Self {
        match error.raw_os_error() {
            Some(code) if code == ERROR_FILE_NOT_FOUND.0 as i32 => Self::NotConfigured,
            Some(code)
                if is_code(
                    code,
                    &[
                        ERROR_NOT_READY.0,
                        ERROR_NO_MEDIA_IN_DRIVE.0,
                        ERROR_PATH_NOT_FOUND.0,
                    ],
                ) =>
            {
                Self::NoDisk
            }
            Some(code) if code == ERROR_UNRECOGNIZED_VOLUME.0 as i32 => Self::Unformatted,
            code => Self::Unreadable(code),
        }
    }
}

/// Result of inspecting the disk in a drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskInspection {
    pub content: DiskContent,
    /// Whether the disk's write-protect tab is set; `None` if unknown.
    pub write_protected: Option<bool>,
}

/// Reads and classifies the disk in `drive`. May take a few seconds right
/// after the disk is inserted, while Windows mounts it.
pub fn inspect_drive(drive: DriveLetter) -> DiskInspection {
    suppress_error_dialogs();
    let content = inspect_dir(Path::new(&drive.root_path()));
    let writable =
        media::writable_by_ioctl(drive).or_else(|| media::writable_by_volume_flag(drive));
    DiskInspection {
        content,
        write_protected: writable.map(|writable| !writable),
    }
}

/// Classifies the configuration found in the folder `root`.
pub fn inspect_dir(root: &Path) -> DiskContent {
    match read_limited(&root.join(FILE_NAME)) {
        Ok(bytes) => DiskContent::from_bytes(&bytes),
        Err(error) => DiskContent::from_read_error(&error),
    }
}

/// Reads at most one byte more than [`MAX_FILE_SIZE`], enough to reject
/// larger files without loading them.
fn read_limited(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_FILE_SIZE as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Write protection as reported by each available source. For diagnostics.
pub fn write_protection_sources(drive: DriveLetter) -> (Option<bool>, Option<bool>) {
    suppress_error_dialogs();
    let protected = |writable: Option<bool>| writable.map(|writable| !writable);
    (
        protected(media::writable_by_ioctl(drive)),
        protected(media::writable_by_volume_flag(drive)),
    )
}

/// Why writing or removing a configuration failed.
#[derive(Debug)]
pub enum WriteError {
    WriteProtected,
    DiskFull,
    /// The disk was removed (or the drive unplugged) during the operation.
    Removed,
    /// The file read back does not match what was written.
    VerifyFailed,
    Io(io::Error),
}

impl WriteError {
    /// Stable code for the error, to be translated by the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Self::WriteProtected => "disk_write_protected",
            Self::DiskFull => "disk_full",
            Self::Removed => "disk_removed",
            Self::VerifyFailed => "disk_write_failed",
            Self::Io(_) => "disk_io",
        }
    }

    fn from_io(error: io::Error) -> Self {
        match error.raw_os_error() {
            Some(code) if code == ERROR_WRITE_PROTECT.0 as i32 => Self::WriteProtected,
            Some(code) if is_code(code, &[ERROR_DISK_FULL.0, ERROR_HANDLE_DISK_FULL.0]) => {
                Self::DiskFull
            }
            Some(code)
                if is_code(
                    code,
                    &[
                        ERROR_NOT_READY.0,
                        ERROR_NO_MEDIA_IN_DRIVE.0,
                        ERROR_MEDIA_CHANGED.0,
                        ERROR_PATH_NOT_FOUND.0,
                    ],
                ) =>
            {
                Self::Removed
            }
            _ => Self::Io(error),
        }
    }
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{}: {error}", self.code()),
            _ => write!(f, "{}", self.code()),
        }
    }
}

impl std::error::Error for WriteError {}

/// Writes `config` to the disk in `drive`. Returns the hash of the written
/// file.
pub fn write_drive(drive: DriveLetter, config: &DiskConfig) -> Result<ConfigHash, WriteError> {
    suppress_error_dialogs();
    write_dir(Path::new(&drive.root_path()), config)
}

/// Writes `config` to the folder `root`. Writes a temporary file first and
/// renames it over the configuration, so a failure (such as the disk being
/// removed) never leaves a half-written configuration; then reads it back
/// to verify it. Returns the hash of the written file.
pub fn write_dir(root: &Path, config: &DiskConfig) -> Result<ConfigHash, WriteError> {
    let text = config.to_toml();
    let temp = root.join(TEMP_FILE_NAME);
    let target = root.join(FILE_NAME);

    let result = write_synced(&temp, text.as_bytes()).and_then(|()| fs::rename(&temp, &target));
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(WriteError::from_io(error));
    }

    let written = read_limited(&target).map_err(WriteError::from_io)?;
    if written != text.as_bytes() {
        return Err(WriteError::VerifyFailed);
    }
    Ok(ConfigHash::of(&written))
}

/// Writes a file and waits until it is on the disk, not just in the cache.
fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Removes the configuration from the disk in `drive`. Returns whether there
/// was one.
pub fn remove_drive(drive: DriveLetter) -> Result<bool, WriteError> {
    suppress_error_dialogs();
    remove_dir(Path::new(&drive.root_path()))
}

/// Removes the configuration from the folder `root`, along with a leftover
/// temporary file. Returns whether there was a configuration.
pub fn remove_dir(root: &Path) -> Result<bool, WriteError> {
    let _ = fs::remove_file(root.join(TEMP_FILE_NAME));
    match fs::remove_file(root.join(FILE_NAME)) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(WriteError::from_io(error)),
    }
}

fn is_code(code: i32, codes: &[u32]) -> bool {
    codes.iter().any(|&c| c as i32 == code)
}

#[cfg(test)]
mod tests;
