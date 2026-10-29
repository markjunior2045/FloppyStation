//! The app's place in Windows: starting with it, and the log file for
//! diagnosing problems.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

use env_logger::{Env, Target};
use floppy_station_core::settings;
use floppy_station_core::startup;

const LOG_FILE: &str = "floppy-station.log";
/// Above this, the log is set aside as `.old` when the app starts, so it
/// never grows without limit.
const MAX_LOG_SIZE: u64 = 1024 * 1024;

/// `%LOCALAPPDATA%\floppy-station\logs`.
fn log_dir() -> Option<PathBuf> {
    Some(settings::local_dir()?.join("logs"))
}

/// Logs to the log file (and the console, when there is one), at the info
/// level unless `RUST_LOG` says otherwise. Panics are logged too.
pub fn init_logging() {
    let mut builder = env_logger::Builder::from_env(Env::default().default_filter_or("info"));
    match open_log() {
        Ok(file) => {
            builder.target(Target::Pipe(Box::new(Tee(file))));
        }
        Err(error) => eprintln!("could not open the log file: {error}"),
    }
    builder.init();

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("{info}");
        default_hook(info);
    }));
}

fn open_log() -> io::Result<File> {
    let dir = log_dir().ok_or_else(|| io::Error::other("no local app data folder"))?;
    fs::create_dir_all(&dir)?;
    let path = dir.join(LOG_FILE);
    if fs::metadata(&path).is_ok_and(|metadata| metadata.len() > MAX_LOG_SIZE) {
        let _ = fs::rename(&path, path.with_extension("log.old"));
    }
    OpenOptions::new().create(true).append(true).open(path)
}

/// Writes to the log file and to the console.
struct Tee(File);

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = io::stderr().write_all(buf);
        self.0.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// Whether Windows starts the app when the user signs in.
#[tauri::command]
pub fn startup_enabled() -> bool {
    std::env::current_exe().is_ok_and(|exe| startup::is_enabled(&exe))
}

#[tauri::command]
pub fn set_startup(enabled: bool) -> Result<(), &'static str> {
    let exe = std::env::current_exe().map_err(|_| "startup_failed")?;
    startup::set_enabled(&exe, enabled).map_err(|error| {
        log::warn!("could not change starting with Windows: {error}");
        "startup_failed"
    })
}

/// Opens the log folder in File Explorer.
#[tauri::command]
pub fn open_log_folder() -> Result<(), &'static str> {
    let dir = log_dir().ok_or("log_folder_missing")?;
    Command::new("explorer")
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|_| "log_folder_missing")
}
