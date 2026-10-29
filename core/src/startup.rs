//! Starting with Windows, through the user's own `Run` registry key, so no
//! administrator rights are needed. The registry is the only record of the
//! choice: Task Manager's Startup tab can also turn the entry off (it marks
//! it in `StartupApproved\Run`), and that counts as off here too.

use std::io;
use std::path::Path;

use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::HSTRING;

use crate::win32::registry_string;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Where Task Manager records entries turned off in its Startup tab.
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const NAME: &str = "Floppy Station";

/// Whether Windows starts `exe` when the user signs in.
pub fn is_enabled(exe: &Path) -> bool {
    let Some(command) = registry_string(HKEY_CURRENT_USER, RUN, NAME) else {
        return false;
    };
    command.eq_ignore_ascii_case(&command_line(exe)) && !turned_off_in_task_manager()
}

/// Makes Windows start `exe` when the user signs in, or stops it.
pub fn set_enabled(exe: &Path, enabled: bool) -> io::Result<()> {
    if enabled {
        let command: Vec<u16> = command_line(exe).encode_utf16().chain([0]).collect();
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(RUN),
                &HSTRING::from(NAME),
                REG_SZ.0,
                Some(command.as_ptr().cast()),
                (command.len() * 2) as u32,
            )
        }
        .ok()?;
    } else {
        delete_value(RUN)?;
    }
    // Turning it on here overrides Task Manager's "off".
    delete_value(APPROVED)
}

fn command_line(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

fn delete_value(subkey: &str) -> io::Result<()> {
    let result = unsafe {
        RegDeleteKeyValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(subkey),
            &HSTRING::from(NAME),
        )
    };
    if result.is_err() && result != ERROR_FILE_NOT_FOUND {
        return Err(io::Error::from_raw_os_error(result.0 as i32));
    }
    Ok(())
}

/// Task Manager stores 12 bytes per entry; an odd first byte means off.
fn turned_off_in_task_manager() -> bool {
    let mut data = [0u8; 12];
    let mut size = data.len() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(APPROVED),
            &HSTRING::from(NAME),
            RRF_RT_REG_BINARY,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    result.is_ok() && size > 0 && data[0] & 1 == 1
}
