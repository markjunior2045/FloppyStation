//! Listing the removable drives, for picking the floppy drive.

use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
use windows::core::HSTRING;

use super::DriveLetter;

/// `DRIVE_REMOVABLE` from `GetDriveTypeW`: floppy drives, USB sticks and card
/// readers.
const DRIVE_REMOVABLE: u32 = 2;

/// The removable drives on this PC. Only asks for each drive's type, which
/// doesn't touch the disk, so empty floppy drives don't click.
pub fn removable_drives() -> Vec<DriveLetter> {
    let mask = unsafe { GetLogicalDrives() };
    ('A'..='Z')
        .enumerate()
        .filter(|(index, _)| mask & (1 << index) != 0)
        .filter_map(|(_, letter)| DriveLetter::parse(&letter.to_string()))
        .filter(|drive| {
            let root = HSTRING::from(drive.root_path());
            let kind = unsafe { GetDriveTypeW(&root) };
            kind == DRIVE_REMOVABLE
        })
        .collect()
}
