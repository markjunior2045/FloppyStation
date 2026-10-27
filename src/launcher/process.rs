//! Finding, asking to close and terminating processes. Only used when a game
//! is being closed: nothing here runs while games are playing.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{FILETIME, HWND, LPARAM, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::JobObjects::{JobObjectBasicProcessIdList, QueryInformationJobObject};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
};
use windows::core::BOOL;

use crate::win32::{OwnedHandle, win32_code};

/// Current time, in the same unit as process creation times (100 ns since
/// 1601).
pub(crate) fn now() -> u64 {
    to_u64(unsafe { GetSystemTimeAsFileTime() })
}

fn to_u64(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

/// A process to be closed. Opened only with rights that also work on
/// elevated processes; holding the handle keeps its ID from being reused.
pub(crate) struct Process {
    pub(crate) pid: u32,
    handle: OwnedHandle,
}

impl Process {
    pub(crate) fn open(pid: u32) -> Result<Self, u32> {
        unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        }
        .map(|handle| Self {
            pid,
            handle: OwnedHandle(handle),
        })
        .map_err(|error| win32_code(&error))
    }

    pub(crate) fn created_at(&self) -> Option<u64> {
        let mut times = [FILETIME::default(); 4];
        let [created, exited, kernel, user] = &mut times;
        unsafe { GetProcessTimes(self.handle.raw(), created, exited, kernel, user) }.ok()?;
        Some(to_u64(times[0]))
    }

    pub(crate) fn is_running(&self) -> bool {
        let result = unsafe { WaitForSingleObject(self.handle.raw(), 0) };
        result == WAIT_TIMEOUT
    }

    /// Waits up to `timeout` for the process to exit. Returns whether it did.
    fn wait(&self, timeout: Duration) -> bool {
        let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        let result = unsafe { WaitForSingleObject(self.handle.raw(), ms) };
        result == WAIT_OBJECT_0
    }

    /// Terminates the process. Fails with access denied on processes running
    /// as administrator.
    pub(crate) fn terminate(&self) -> Result<(), u32> {
        let handle = unsafe { OpenProcess(PROCESS_TERMINATE, false, self.pid) }
            .map(OwnedHandle)
            .map_err(|error| win32_code(&error))?;
        unsafe { TerminateProcess(handle.raw(), 1) }.map_err(|error| win32_code(&error))
    }
}

/// Waits up to `timeout` for all `processes` to exit. Returns whether they
/// did.
pub(crate) fn wait_all(processes: &[Process], timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    processes
        .iter()
        .all(|process| process.wait(deadline.saturating_duration_since(Instant::now())))
}

/// IDs of the running processes whose executable is named `name` (e.g.
/// `cs2.exe`), case-insensitive.
pub(crate) fn processes_named(name: &str) -> Vec<u32> {
    let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
        Ok(snapshot) => OwnedHandle(snapshot),
        Err(error) => {
            log::warn!("could not list processes: {error}");
            return Vec::new();
        }
    };
    let name = name.to_lowercase();
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut pids = Vec::new();
    let mut more = unsafe { Process32FirstW(snapshot.raw(), &mut entry) }.is_ok();
    while more {
        let file = &entry.szExeFile;
        let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
        if String::from_utf16_lossy(&file[..len]).to_lowercase() == name {
            pids.push(entry.th32ProcessID);
        }
        more = unsafe { Process32NextW(snapshot.raw(), &mut entry) }.is_ok();
    }
    pids
}

/// IDs of the processes in a job.
pub(crate) fn job_pids(job: &OwnedHandle) -> Vec<u32> {
    const MAX: usize = 256;
    // Same layout as JOBOBJECT_BASIC_PROCESS_ID_LIST, with room for MAX IDs.
    #[repr(C)]
    struct List {
        assigned: u32,
        listed: u32,
        ids: [usize; MAX],
    }
    let mut list = List {
        assigned: 0,
        listed: 0,
        ids: [0; MAX],
    };
    let result = unsafe {
        QueryInformationJobObject(
            Some(job.raw()),
            JobObjectBasicProcessIdList,
            (&raw mut list).cast(),
            size_of::<List>() as u32,
            None,
        )
    };
    if let Err(error) = result {
        log::warn!("could not list the processes of the game: {error}");
        return Vec::new();
    }
    let listed = (list.listed as usize).min(MAX);
    list.ids[..listed].iter().map(|&id| id as u32).collect()
}

/// Sends a close request (like clicking the window's X) to every visible
/// top-level window of the processes `pids`. Returns how many windows got it.
pub(crate) fn ask_to_close(pids: &BTreeSet<u32>) -> usize {
    struct Search<'a> {
        pids: &'a BTreeSet<u32>,
        windows: Vec<HWND>,
    }

    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if search.pids.contains(&pid) && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            search.windows.push(hwnd);
        }
        true.into()
    }

    let mut search = Search {
        pids,
        windows: Vec::new(),
    };
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&raw mut search as isize)) };
    for &hwnd in &search.windows {
        // Fails for windows of processes running as administrator.
        if let Err(error) = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
            log::debug!("could not ask window {hwnd:?} to close: {error}");
        }
    }
    search.windows.len()
}
