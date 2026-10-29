//! Small Win32 helpers shared by the modules.

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Registry::{HKEY, RRF_RT_REG_SZ, RegGetValueW};
use windows::core::HSTRING;

use crate::floppy::DriveLetter;

/// A handle closed when dropped.
pub(crate) struct OwnedHandle(pub(crate) HANDLE);

// Kernel handles can be used and closed from any thread.
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}

impl OwnedHandle {
    pub(crate) fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Opens the volume device of a drive (e.g. `\\.\A:`) with no access rights.
/// Enough for status queries such as checking for a disk; needs no
/// administrator permission and does not touch the file system. Returns the
/// Win32 error code on failure.
pub(crate) fn open_volume(drive: DriveLetter) -> Result<OwnedHandle, u32> {
    open_volume_with(drive, 0)
}

/// Opens the volume device of a drive with the given access rights.
pub(crate) fn open_volume_with(drive: DriveLetter, access: u32) -> Result<OwnedHandle, u32> {
    let path = HSTRING::from(drive.device_path());
    unsafe {
        CreateFileW(
            &path,
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map(OwnedHandle)
    .map_err(|error| win32_code(&error))
}

/// Extracts the Win32 error code from a `windows` error, which wraps it in an
/// HRESULT (`0x8007xxxx`).
pub(crate) fn win32_code(error: &windows::core::Error) -> u32 {
    let hresult = error.code().0 as u32;
    if hresult & 0xFFFF_0000 == 0x8007_0000 {
        hresult & 0xFFFF
    } else {
        hresult
    }
}

/// A text value from the registry, or `None` if it is missing or not text.
pub(crate) fn registry_string(key: HKEY, subkey: &str, value: &str) -> Option<String> {
    let (subkey, value) = (HSTRING::from(subkey), HSTRING::from(value));
    let mut buffer = [0u16; 1024];
    let mut size = size_of_val(&buffer) as u32;
    let result = unsafe {
        RegGetValueW(
            key,
            &subkey,
            &value,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    if result.is_err() {
        return None;
    }
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]))
}
