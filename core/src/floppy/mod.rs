//! Floppy drive monitoring: checks whether a disk is present in each watched
//! drive and reports insertion/removal as [`FloppyEvent`]s.
//!
//! This module only reports facts about the drives. Deciding what to do with
//! them (launching or closing games) belongs to a higher layer.

mod monitor;
mod probe;

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

mod drives;

pub use drives::removable_drives;
pub use monitor::{Monitor, PollIntervals, Poller, suppress_error_dialogs};
pub use probe::{IoctlProbe, MediaProbe, ProbeKind, ProbeResult, VolumeProbe};

/// A drive letter, always stored in uppercase (`'A'`..=`'Z'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DriveLetter(char);

impl DriveLetter {
    /// Parses `"A"`, `"a:"`, `"A:\"` or `"A:/"` (surrounding whitespace allowed).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let rest = text
            .strip_suffix(":\\")
            .or_else(|| text.strip_suffix(":/"))
            .or_else(|| text.strip_suffix(':'))
            .unwrap_or(text);
        let mut chars = rest.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) if c.is_ascii_alphabetic() => Some(Self(c.to_ascii_uppercase())),
            _ => None,
        }
    }

    pub fn as_char(self) -> char {
        self.0
    }

    /// Volume device path used to talk to the drive without touching the
    /// file system, e.g. `\\.\A:`.
    pub fn device_path(self) -> String {
        format!(r"\\.\{}:", self.0)
    }

    /// Root directory of the volume, e.g. `A:\`.
    pub fn root_path(self) -> String {
        format!(r"{}:\", self.0)
    }
}

impl Default for DriveLetter {
    fn default() -> Self {
        Self('A')
    }
}

impl fmt::Display for DriveLetter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:", self.0)
    }
}

impl Serialize for DriveLetter {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for DriveLetter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid drive letter: {text:?}")))
    }
}

/// Information about the disk currently in a drive.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaInfo {
    /// Volume serial number; `None` if the disk is unformatted or unreadable.
    pub serial: Option<u32>,
    /// Volume label; `None` if empty, unformatted or unreadable.
    pub label: Option<String>,
}

/// What is currently known about a drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveStatus {
    /// The drive letter does not exist (drive unplugged or wrong letter).
    Missing,
    /// The drive is there, but has no disk.
    Empty,
    /// The drive has a disk.
    Present,
}

/// How a disk came to be reported as present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOrigin {
    /// The disk was inserted while monitoring.
    Inserted,
    /// The drive was plugged back in with a disk inside. Counts as an
    /// insertion.
    DriveReconnected,
    /// The disk was already in the drive when monitoring started.
    PresentAtStartup,
}

/// Why a disk stopped being reported as present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalCause {
    /// The disk was taken out of the drive.
    Ejected,
    /// The whole drive went away with the disk inside.
    DriveDisconnected,
}

/// A change in the state of a watched drive.
///
/// The first poll of each drive always reports its initial state:
/// `DriveDisconnected` if it is missing, `DriveConnected` otherwise, followed
/// by `MediaInserted { origin: PresentAtStartup }` if it has a disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FloppyEvent {
    DriveConnected {
        drive: DriveLetter,
    },
    DriveDisconnected {
        drive: DriveLetter,
    },
    /// A disk is in the drive. Its volume is not read here: right after a
    /// swap, Windows may still report the previous disk's serial and files
    /// (see [`crate::diskconfig::inspect_inserted`]).
    MediaInserted {
        drive: DriveLetter,
        origin: InsertOrigin,
    },
    MediaRemoved {
        drive: DriveLetter,
        cause: RemovalCause,
    },
}

impl FloppyEvent {
    /// Stable code for the event, to be translated by the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Self::DriveConnected { .. } => "drive_connected",
            Self::DriveDisconnected { .. } => "drive_disconnected",
            Self::MediaInserted { .. } => "media_inserted",
            Self::MediaRemoved { .. } => "media_removed",
        }
    }

    pub fn drive(&self) -> DriveLetter {
        match self {
            Self::DriveConnected { drive }
            | Self::DriveDisconnected { drive }
            | Self::MediaInserted { drive, .. }
            | Self::MediaRemoved { drive, .. } => *drive,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_drive_letter_variants() {
        for text in ["A", "a", "A:", "a:", r"A:\", "A:/", "  a:  "] {
            assert_eq!(DriveLetter::parse(text), Some(DriveLetter('A')), "{text:?}");
        }
    }

    #[test]
    fn rejects_invalid_drive_letters() {
        for text in ["", ":", "AB", "AB:", "1:", "é", r"A:\foo", "A::"] {
            assert_eq!(DriveLetter::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn formats_paths() {
        let drive = DriveLetter::parse("b").unwrap();
        assert_eq!(drive.to_string(), "B:");
        assert_eq!(drive.device_path(), r"\\.\B:");
        assert_eq!(drive.root_path(), r"B:\");
    }
}
