//! Low-level checks of a single drive, kept behind the [`MediaProbe`] trait so
//! the monitor's state machine can be tested without hardware.

use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_INVALID_DRIVE, ERROR_MEDIA_CHANGED, ERROR_NO_MEDIA_IN_DRIVE,
    ERROR_NOT_READY, ERROR_PATH_NOT_FOUND, ERROR_UNRECOGNIZED_VOLUME,
};
use windows::Win32::Storage::FileSystem::GetVolumeInformationW;
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::IOCTL_STORAGE_CHECK_VERIFY2;
use windows::core::HSTRING;

use super::{DriveLetter, MediaInfo};
use crate::win32::{open_volume, win32_code};

/// Result of a single presence check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeResult {
    /// The drive letter does not exist.
    DriveMissing,
    /// The drive exists but has no disk.
    NoMedia,
    /// The drive has a disk. `change_token`, when available, changes whenever
    /// the disk is swapped, so a swap faster than the polling interval can
    /// still be noticed.
    Media { change_token: Option<u64> },
    /// The check failed in an unexpected way (raw Win32 error code). The
    /// monitor keeps the previous state.
    Unknown(u32),
}

/// A way of checking a drive for a disk.
pub trait MediaProbe: Send {
    /// Cheap check, run on every poll.
    fn probe(&mut self, drive: DriveLetter) -> ProbeResult;

    /// Reads details of the disk. Only called when a disk shows up, since it
    /// may access the disk itself. Retries once if the drive is momentarily
    /// not ready, which happens right after a disk change.
    fn read_media_info(&mut self, drive: DriveLetter) -> MediaInfo {
        match read_volume(drive) {
            Ok(info) => info,
            Err(code) if is_no_media(code) => {
                thread::sleep(READ_RETRY_DELAY);
                read_volume(drive).unwrap_or_default()
            }
            Err(_) => MediaInfo::default(),
        }
    }
}

const READ_RETRY_DELAY: Duration = Duration::from_millis(500);

/// Available probe implementations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProbeKind {
    #[default]
    Ioctl,
    Volume,
}

impl ProbeKind {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "ioctl" => Some(Self::Ioctl),
            "volume" => Some(Self::Volume),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Ioctl => "ioctl",
            Self::Volume => "volume",
        }
    }

    pub fn create(self) -> Box<dyn MediaProbe> {
        match self {
            Self::Ioctl => Box::new(IoctlProbe),
            Self::Volume => Box::new(VolumeProbe),
        }
    }
}

/// Asks the storage driver whether a disk is present
/// (`IOCTL_STORAGE_CHECK_VERIFY2`), without reading any sector or touching the
/// file system. The volume is opened with no access rights, so this needs no
/// administrator permission.
pub struct IoctlProbe;

impl MediaProbe for IoctlProbe {
    fn probe(&mut self, drive: DriveLetter) -> ProbeResult {
        let handle = match open_volume(drive) {
            Ok(handle) => handle,
            Err(code) => {
                return match code {
                    code if is_missing_drive(code) => {
                        log::trace!("drive {drive} open failed with error {code}");
                        ProbeResult::DriveMissing
                    }
                    code => ProbeResult::Unknown(code),
                };
            }
        };

        let mut change_count = 0u32;
        let mut returned = 0u32;
        let result = unsafe {
            DeviceIoControl(
                handle.raw(),
                IOCTL_STORAGE_CHECK_VERIFY2,
                None,
                0,
                Some((&raw mut change_count).cast()),
                size_of::<u32>() as u32,
                Some(&raw mut returned),
                None,
            )
        };

        match result {
            Ok(()) => ProbeResult::Media {
                change_token: (returned as usize >= size_of::<u32>())
                    .then_some(u64::from(change_count)),
            },
            Err(error) => match win32_code(&error) {
                code if is_no_media(code) => {
                    log::trace!("drive {drive} check failed with error {code}");
                    ProbeResult::NoMedia
                }
                code if code == ERROR_MEDIA_CHANGED.0 => ProbeResult::Media { change_token: None },
                code if is_missing_drive(code) => ProbeResult::DriveMissing,
                code => ProbeResult::Unknown(code),
            },
        }
    }
}

/// Reads the volume information (`GetVolumeInformationW`) on every poll.
/// Heavier than [`IoctlProbe`], since it mounts and reads the file system,
/// but works with any driver. Uses the volume serial number as change token.
pub struct VolumeProbe;

impl MediaProbe for VolumeProbe {
    fn probe(&mut self, drive: DriveLetter) -> ProbeResult {
        match read_volume(drive) {
            Ok(info) => ProbeResult::Media {
                change_token: info.serial.map(u64::from),
            },
            Err(code) if code == ERROR_UNRECOGNIZED_VOLUME.0 => {
                ProbeResult::Media { change_token: None }
            }
            Err(code) if is_no_media(code) => ProbeResult::NoMedia,
            Err(code) if is_missing_drive(code) => ProbeResult::DriveMissing,
            Err(code) => ProbeResult::Unknown(code),
        }
    }
}

/// Reads serial number and label of the volume. An unformatted disk yields
/// empty info instead of an error.
fn read_volume(drive: DriveLetter) -> Result<MediaInfo, u32> {
    let root = HSTRING::from(drive.root_path());
    let mut label = [0u16; 64];
    let mut serial = 0u32;
    let result = unsafe {
        GetVolumeInformationW(
            &root,
            Some(&mut label),
            Some(&raw mut serial),
            None,
            None,
            None,
        )
    };
    match result {
        Ok(()) => {
            let len = label.iter().position(|&c| c == 0).unwrap_or(label.len());
            let label = String::from_utf16_lossy(&label[..len]).trim().to_owned();
            let info = MediaInfo {
                serial: Some(serial),
                label: (!label.is_empty()).then_some(label),
            };
            Ok(info)
        }
        Err(error) => Err(win32_code(&error)),
    }
}

fn is_no_media(code: u32) -> bool {
    code == ERROR_NOT_READY.0 || code == ERROR_NO_MEDIA_IN_DRIVE.0
}

fn is_missing_drive(code: u32) -> bool {
    code == ERROR_FILE_NOT_FOUND.0
        || code == ERROR_PATH_NOT_FOUND.0
        || code == ERROR_INVALID_DRIVE.0
}
