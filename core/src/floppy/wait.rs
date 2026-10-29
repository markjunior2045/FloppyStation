//! Quiet detection for empty drives.
//!
//! Asking an empty floppy drive whether it has a disk makes it click, so
//! polling once a second clicks once a second. But when Windows tries to
//! mount an empty floppy drive (opening its volume with read access), the
//! request stays pending until a disk is inserted, and the drive only
//! retries about every 20 s. Keeping one such request pending turns the
//! clicking into one click every ~20 s, and notices disks the moment they
//! go in.
//!
//! That pending request can't be cancelled: neither cancelling the I/O nor
//! killing the process takes effect before Windows gives up (minutes
//! later), and meanwhile every other request to the drive waits behind it.
//! So it runs in a small helper process (a copy of the app started with
//! [`WAIT_ARG`]) that the monitor never has to wait for.
//!
//! Each helper holds a named mutex for its drive until it ends. A monitor
//! that finds the mutex taken knows the drive is empty and takes over that
//! wait instead of checking the drive, which would only queue behind it.
//! This covers a monitor replaced by another (detection turned off and on)
//! and an app started again while the previous one's helper still waits.
//! The helpers are tied to a job object, so they go away with the app (once
//! Windows lets them).

use std::fs;
use std::io;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use windows::Win32::Foundation::{GENERIC_READ, HANDLE, WAIT_TIMEOUT};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CreateMutexW, MUTEX_MODIFY_STATE, OpenMutexW, ReleaseMutex,
    SYNCHRONIZATION_SYNCHRONIZE, WaitForSingleObject,
};
use windows::core::{HSTRING, PCWSTR};

use super::{DriveLetter, suppress_error_dialogs};
use crate::settings;
use crate::win32::{OwnedHandle, open_volume_with};

/// Command-line argument that turns the app into a wait helper:
/// `<exe> --wait-disk A:`.
pub const WAIT_ARG: &str = "--wait-disk";

/// Starts waits for a disk in an empty drive.
pub trait DiskWaiter: Send {
    fn start(&mut self, drive: DriveLetter) -> io::Result<Box<dyn PendingWait>>;

    /// A wait for `drive` started earlier (by another monitor or app
    /// instance) and still running, if any. The drive is then known to be
    /// empty without checking it.
    fn resume(&mut self, _drive: DriveLetter) -> Option<Box<dyn PendingWait>> {
        None
    }
}

/// A wait in progress. Dropping it abandons the wait.
pub trait PendingWait: Send {
    /// Whether the wait ended: a disk went in, the drive was unplugged, or
    /// Windows gave up. The caller checks the drive to know which.
    fn finished(&mut self) -> bool;
}

/// Waits through helper processes running `exe --wait-disk <drive>`.
#[derive(Clone)]
pub struct HelperWaiter {
    exe: PathBuf,
    /// Kills the helpers when the app exits; `None` if it couldn't be set
    /// up. Shared by clones, so it lives as long as the app uses any.
    job: Arc<Option<OwnedHandle>>,
}

impl HelperWaiter {
    /// Helpers running the current executable, which must call
    /// [`run_wait_helper`] at startup. They run from a copy of it (see
    /// [`helper_copy`]).
    pub fn current_exe() -> io::Result<Self> {
        Ok(Self {
            exe: helper_copy(&std::env::current_exe()?),
            job: Arc::new(kill_on_close_job()),
        })
    }
}

/// A copy of `exe` for the helpers to run from, in the local app folder. A
/// helper can outlive the app by minutes (until Windows gives up on its
/// request), and while it runs its executable can't be replaced: running
/// from a copy keeps the app's own executable free to update, uninstall or
/// rebuild. Falls back to `exe` itself if the copy can't be made.
fn helper_copy(exe: &Path) -> PathBuf {
    match try_helper_copy(exe) {
        Ok(copy) => copy,
        Err(error) => {
            log::warn!("could not copy the wait helper, running it from the app: {error}");
            exe.to_path_buf()
        }
    }
}

fn try_helper_copy(exe: &Path) -> io::Result<PathBuf> {
    let dir = settings::local_dir()
        .ok_or_else(|| io::Error::other("no local app data folder"))?
        .join("helper");
    fs::create_dir_all(&dir)?;
    // Named after the build, so a copy still in use is never overwritten.
    let metadata = fs::metadata(exe)?;
    let modified = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let stem = exe
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("floppy-station");
    let prefix = format!("{stem}-wait-");
    let copy = dir.join(format!("{prefix}{}-{modified}.exe", metadata.len()));
    if !copy.exists() {
        let temp = copy.with_extension("tmp");
        fs::copy(exe, &temp)?;
        fs::rename(&temp, &copy)?;
    }
    // Copies of other builds go, unless a helper still runs from them.
    for entry in fs::read_dir(&dir)?.flatten() {
        let path = entry.path();
        if path != copy && entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = fs::remove_file(path);
        }
    }
    Ok(copy)
}

impl DiskWaiter for HelperWaiter {
    fn start(&mut self, drive: DriveLetter) -> io::Result<Box<dyn PendingWait>> {
        if let Some(wait) = self.resume(drive) {
            return Ok(wait);
        }
        let child = Command::new(&self.exe)
            .args([WAIT_ARG, &drive.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW.0)
            .spawn()?;
        if let Some(job) = self.job.as_ref() {
            use std::os::windows::io::AsRawHandle;
            let process = HANDLE(child.as_raw_handle());
            if let Err(error) = unsafe { AssignProcessToJobObject(job.raw(), process) } {
                log::debug!("could not put the wait helper in the job: {error}");
            }
        }
        log::debug!(
            "waiting for a disk in drive {drive} (helper {})",
            child.id()
        );
        Ok(Box::new(Helper(child)))
    }

    fn resume(&mut self, drive: DriveLetter) -> Option<Box<dyn PendingWait>> {
        let access = SYNCHRONIZATION_SYNCHRONIZE | MUTEX_MODIFY_STATE;
        let mutex = unsafe { OpenMutexW(access, false, &mutex_name(drive)) }.ok()?;
        let mut wait = Running(OwnedHandle(mutex));
        if wait.finished() {
            return None;
        }
        log::debug!("taking over the wait for a disk in drive {drive}");
        Some(Box::new(wait))
    }
}

/// A helper this monitor started. Dropping it leaves the helper running:
/// killing it wouldn't free the drive any sooner, and the next monitor of
/// the drive takes it over.
struct Helper(Child);

impl PendingWait for Helper {
    fn finished(&mut self) -> bool {
        !matches!(self.0.try_wait(), Ok(None))
    }
}

/// A helper started elsewhere, followed through its mutex.
struct Running(OwnedHandle);

impl PendingWait for Running {
    fn finished(&mut self) -> bool {
        // The mutex is free (or abandoned) once the helper has ended.
        if unsafe { WaitForSingleObject(self.0.raw(), 0) } == WAIT_TIMEOUT {
            return false;
        }
        let _ = unsafe { ReleaseMutex(self.0.raw()) };
        true
    }
}

/// Name of the mutex a helper holds while waiting on `drive`. Per session,
/// like the app.
fn mutex_name(drive: DriveLetter) -> HSTRING {
    HSTRING::from(format!(r"Local\floppy-station-wait-{}", drive.as_char()))
}

fn kill_on_close_job() -> Option<OwnedHandle> {
    let job = match unsafe { CreateJobObjectW(None, PCWSTR::null()) } {
        Ok(job) => OwnedHandle(job),
        Err(error) => {
            log::warn!("could not create a job for the wait helpers: {error}");
            return None;
        }
    };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let result = unsafe {
        SetInformationJobObject(
            job.raw(),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    match result {
        Ok(()) => Some(job),
        Err(error) => {
            log::warn!("could not set up the job for the wait helpers: {error}");
            None
        }
    }
}

/// Runs the wait helper if the process was started as one, and returns
/// whether it was. Call first thing in `main` and exit when it returns
/// `true`.
pub fn run_wait_helper() -> bool {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some(WAIT_ARG) {
        return false;
    }
    if let Some(drive) = args.next().as_deref().and_then(DriveLetter::parse) {
        suppress_error_dialogs();
        // Held until this process ends; see the module documentation.
        let _mutex = unsafe { CreateMutexW(None, true, &mutex_name(drive)) };
        // Blocks until a disk is in, the drive is gone, or Windows gives up.
        // The result doesn't matter: the monitor checks the drive anyway.
        drop(open_volume_with(drive, GENERIC_READ.0));
    }
    true
}
