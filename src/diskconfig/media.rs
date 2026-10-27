//! Write protection checks of the disk in a drive.

use windows::Win32::Foundation::ERROR_WRITE_PROTECT;
use windows::Win32::Storage::FileSystem::GetVolumeInformationW;
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::IOCTL_DISK_IS_WRITABLE;
use windows::core::HSTRING;

use crate::floppy::DriveLetter;
use crate::win32::{open_volume, win32_code};

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
                "drive {drive}: IOCTL_DISK_IS_WRITABLE failed with error {}",
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
