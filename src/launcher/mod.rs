//! Launching games and closing them when their disk is removed.
//!
//! Each kind launches its own way (see [`crate::kinds`]); closing is common
//! to all kinds. Only processes the app started are closed: the ones it
//! created itself, plus the ones named by the config's `process` that were
//! started after the launch. Processes are never watched while the game
//! runs; they are only looked up when the game is closed.

mod process;

use std::collections::BTreeSet;
use std::fmt;
use std::time::Duration;

use windows::Win32::System::JobObjects::TerminateJobObject;
use windows::Win32::System::Threading::{GetCurrentProcessId, GetProcessId};

use crate::diskconfig::DiskConfig;
use crate::win32::OwnedHandle;
use process::Process;

/// How long to wait for processes to exit after forcing them to close.
const FORCE_WAIT: Duration = Duration::from_secs(5);

/// Why a game could not be launched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchError {
    /// The executable does not exist on this PC.
    NotFound,
    /// The executable requires administrator rights. The app never elevates,
    /// since it could not close a game running as administrator.
    ElevationRequired,
    /// Steam is not installed (nothing handles `steam://` links).
    SteamMissing,
    /// Any other failure, with the Win32 error code.
    Failed(u32),
}

impl LaunchError {
    /// Stable code for the error, to be translated by the UI.
    pub fn code(self) -> &'static str {
        match self {
            Self::NotFound => "launch_not_found",
            Self::ElevationRequired => "launch_elevation_required",
            Self::SteamMissing => "launch_steam_missing",
            Self::Failed(_) => "launch_failed",
        }
    }
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed(code) => write!(f, "{} (error {code})", self.code()),
            _ => write!(f, "{}", self.code()),
        }
    }
}

impl std::error::Error for LaunchError {}

/// What a kind's launch started that can be closed later.
#[derive(Default)]
pub(crate) struct Started {
    /// Job holding the started process and the processes it starts.
    pub(crate) job: Option<OwnedHandle>,
    /// The started process itself.
    pub(crate) process: Option<OwnedHandle>,
}

/// How to close a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseMode {
    /// Ask the game to close, and force it if it is still open after the
    /// timeout.
    Graceful(Duration),
    /// Force the game to close right away.
    Force,
}

/// Result of closing a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    /// Nothing to close: the game was already closed.
    AlreadyClosed,
    /// The game closed when asked.
    Closed,
    /// The game was forced to close.
    Forced,
    /// Some processes could not be closed (for example, running as
    /// administrator).
    Failed { remaining: usize },
}

impl CloseOutcome {
    /// Stable code for the outcome, to be translated by the UI.
    pub fn code(self) -> &'static str {
        match self {
            Self::AlreadyClosed => "close_already_closed",
            Self::Closed => "close_done",
            Self::Forced => "close_forced",
            Self::Failed { .. } => "close_failed",
        }
    }
}

/// A game launched by the app.
pub struct RunningGame {
    started: Started,
    /// The config's `process`.
    process_name: Option<String>,
    /// When the game was launched; processes found by name that are older
    /// were not started by the app and are left alone.
    launched_at: u64,
}

impl RunningGame {
    pub fn launch(config: &DiskConfig) -> Result<Self, LaunchError> {
        let launched_at = process::now();
        let started = config.target.launch()?;
        Ok(Self {
            started,
            process_name: config.process.clone(),
            launched_at,
        })
    }

    /// Closes the game. Blocks until it is closed or the timeouts expire, so
    /// it should run on its own thread.
    pub fn close(self, mode: CloseMode) -> CloseOutcome {
        let processes = self.find_processes();
        if processes.is_empty() {
            return CloseOutcome::AlreadyClosed;
        }
        let pids: BTreeSet<u32> = processes.iter().map(|process| process.pid).collect();
        log::debug!("closing processes {pids:?}");

        if let CloseMode::Graceful(timeout) = mode {
            let windows = process::ask_to_close(&pids);
            log::debug!("asked {windows} windows to close");
            if process::wait_all(&processes, timeout) {
                return CloseOutcome::Closed;
            }
            log::info!("the game did not close within {timeout:?}, forcing it");
        }

        if let Some(job) = &self.started.job
            && let Err(error) = unsafe { TerminateJobObject(job.raw(), 1) }
        {
            log::warn!("could not terminate the game's job: {error}");
        }
        for process in processes.iter().filter(|process| process.is_running()) {
            if let Err(code) = process.terminate() {
                log::warn!("could not terminate process {} (error {code})", process.pid);
            }
        }
        process::wait_all(&processes, FORCE_WAIT);
        match processes
            .iter()
            .filter(|process| process.is_running())
            .count()
        {
            0 => CloseOutcome::Forced,
            remaining => CloseOutcome::Failed { remaining },
        }
    }

    /// The running processes that belong to the game.
    fn find_processes(&self) -> Vec<Process> {
        let mut pids = BTreeSet::new();
        if let Some(job) = &self.started.job {
            pids.extend(process::job_pids(job));
        }
        if let Some(handle) = &self.started.process {
            pids.insert(unsafe { GetProcessId(handle.raw()) });
        }
        let mut processes: Vec<Process> = pids
            .iter()
            .filter_map(|&pid| Process::open(pid).ok())
            .collect();

        if let Some(name) = &self.process_name {
            for pid in process::processes_named(name) {
                if pids.contains(&pid) {
                    continue;
                }
                match Process::open(pid) {
                    Ok(process) if process.created_at() >= Some(self.launched_at) => {
                        processes.push(process);
                    }
                    Ok(_) => log::info!(
                        "leaving {name} ({pid}) open: it was running before the game was launched"
                    ),
                    Err(code) => {
                        log::warn!("could not open {name} ({pid}), error {code}; leaving it open")
                    }
                }
            }
        }

        let own = unsafe { GetCurrentProcessId() };
        processes.retain(|process| process.pid != own && process.is_running());
        processes
    }
}
