//! Volume-level operations on the disk in a drive: write protection checks
//! and making Windows read the disk again.

use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{ERROR_WRITE_PROTECT, GENERIC_READ};
use windows::Win32::Storage::FileSystem::GetVolumeInformationW;
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    DISK_GEOMETRY, FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, FixedMedia,
    IOCTL_DISK_GET_DRIVE_GEOMETRY, IOCTL_DISK_IS_WRITABLE, RemovableMedia, Unknown,
};
use windows::core::HSTRING;

use crate::floppy::{DriveLetter, IoctlProbe, MediaProbe, ProbeResult};
use crate::win32::{OwnedHandle, open_volume, open_volume_with, win32_code};

/// `FILE_READ_ONLY_VOLUME` file system flag.
const FILE_READ_ONLY_VOLUME: u32 = 0x0008_0000;

/// Asks the disk driver whether the disk can be written
/// (`IOCTL_DISK_IS_WRITABLE`). `None` if the driver could not tell.
pub(crate) fn writable_by_ioctl(drive: DriveLetter) -> Option<bool> {
    let handle = open_volume(drive).ok()?;
    let result = unsafe {
        DeviceIoControl(
            handle.raw(),
            IOCTL_DISK_IS_WRITABLE,
            None,
            0,
            None,
            0,
            None,
            None,
        )
    };
    match result {
        Ok(()) => Some(true),
        Err(error) if win32_code(&error) == ERROR_WRITE_PROTECT.0 => Some(false),
        Err(error) => {
            log::debug!(
                "IOCTL_DISK_IS_WRITABLE on {drive} failed with error {}",
                win32_code(&error)
            );
            None
        }
    }
}

/// Reads the read-only flag of the mounted file system. `None` if the
/// volume could not be read.
pub(crate) fn writable_by_volume_flag(drive: DriveLetter) -> Option<bool> {
    let root = HSTRING::from(drive.root_path());
    let mut flags = 0u32;
    unsafe { GetVolumeInformationW(&root, None, None, None, Some(&raw mut flags), None) }.ok()?;
    Some(flags & FILE_READ_ONLY_VOLUME == 0)
}

/// How to make Windows forget a disk it may have confused with another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refresh {
    /// Right after an insertion: dismount even if the volume is in use,
    /// since anything still using it belongs to the disk that was removed.
    Force,
    /// Any other time: only if nothing is using the volume.
    IfIdle,
}

/// Why the volume was not refreshed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefreshError {
    /// No disk in the drive: nothing to refresh.
    NoDisk,
    /// Not a floppy drive: left alone.
    NotFloppy,
    /// Something is using the volume (with [`Refresh::IfIdle`]).
    InUse,
    /// The volume could not be opened or dismounted, with the Win32 code.
    Failed(u32),
}

/// Dismounts the volume of a floppy drive, so that Windows reads the disk
/// again instead of what it remembers. Some USB floppy drives do not always
/// tell Windows that the disk was swapped, and Windows then keeps showing
/// the files of the previous disk (and would write with them). Writes
/// nothing to the disk; drives other than floppy drives are never touched.
pub(crate) fn refresh_volume(drive: DriveLetter, mode: Refresh) -> Result<(), RefreshError> {
    // The light check used for detection first: on an empty drive, the
    // queries below make the drive retry reading for minutes.
    if !has_disk(drive) {
        return Err(RefreshError::NoDisk);
    }
    let handle = open_volume_with(drive, GENERIC_READ.0).map_err(RefreshError::Failed)?;
    if !is_floppy(&handle) {
        return Err(RefreshError::NotFloppy);
    }
    // Locking succeeds only if nothing is using the volume, and makes
    // Windows finish pending writes first. Closing the handle unlocks it.
    if let Err(error) = ioctl(&handle, FSCTL_LOCK_VOLUME) {
        if mode == Refresh::IfIdle {
            return Err(RefreshError::InUse);
        }
        log::debug!(
            "volume {drive} in use (error {}), dismounting anyway",
            win32_code(&error)
        );
    }
    ioctl(&handle, FSCTL_DISMOUNT_VOLUME).map_err(|error| RefreshError::Failed(win32_code(&error)))
}

/// Whether there is a disk in the drive. Right after a disk swap, the first
/// checks report "not ready" just like an empty drive, so "no disk" is only
/// trusted after a few checks.
fn has_disk(drive: DriveLetter) -> bool {
    const CHECKS: usize = 4;
    const DELAY: Duration = Duration::from_millis(250);
    for check in 0..CHECKS {
        match IoctlProbe.probe(drive) {
            ProbeResult::Media { .. } => return true,
            ProbeResult::DriveMissing => return false,
            result => log::debug!("volume {drive} check {} gave {result:?}", check + 1),
        }
        if check + 1 < CHECKS {
            thread::sleep(DELAY);
        }
    }
    false
}

/// Whether the disk in the drive is a floppy disk, from the media type the
/// disk driver reports.
fn is_floppy(handle: &OwnedHandle) -> bool {
    let mut geometry = DISK_GEOMETRY::default();
    let mut returned = 0;
    let result = unsafe {
        DeviceIoControl(
            handle.raw(),
            IOCTL_DISK_GET_DRIVE_GEOMETRY,
            None,
            0,
            Some((&raw mut geometry).cast()),
            size_of::<DISK_GEOMETRY>() as u32,
            Some(&mut returned),
            None,
        )
    };
    // Every media type other than these is a floppy format.
    result.is_ok() && ![Unknown, RemovableMedia, FixedMedia].contains(&geometry.MediaType)
}

fn ioctl(handle: &OwnedHandle, code: u32) -> windows::core::Result<()> {
    let mut returned = 0;
    unsafe {
        DeviceIoControl(
            handle.raw(),
            code,
            None,
            0,
            None,
            0,
            Some(&mut returned),
            None,
        )
    }
}
