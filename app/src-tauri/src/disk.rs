//! Commands of the Disk and Approvals tabs: the lists to pick a game or a
//! process from, writing and removing the disk's configuration, and
//! approving programs.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use floppy_station_core::diskconfig::{
    self, ConfigError, DiskConfig, DiskContent, ParseError, WriteError,
};
use floppy_station_core::kinds::steam::{self, LibraryGame};
use floppy_station_core::kinds::{ExeGame, GameTarget, SteamGame, exe};
use floppy_station_core::launcher;
use floppy_station_core::trust::{self, RevokeError, TrustEntry, TrustStore};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::events;
use crate::watcher::Watcher;

/// Tries when another program is using the disk (an Explorer window on it,
/// say), which often lasts only a moment.
const IN_USE_TRIES: usize = 3;
const IN_USE_RETRY: Duration = Duration::from_secs(1);

/// The Disk tab's form.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigForm {
    /// `steam` or `exe`.
    kind: String,
    name: String,
    app_id: Option<u32>,
    path: String,
    /// Arguments as typed, on one line.
    args: String,
    process: String,
}

/// Why a command failed: a code for the language file, with its data.
#[derive(Debug, Serialize)]
pub struct Problem {
    code: &'static str,
    params: BTreeMap<&'static str, String>,
}

impl Problem {
    fn new(code: &'static str) -> Self {
        Self {
            code,
            params: BTreeMap::new(),
        }
    }

    fn with(mut self, key: &'static str, value: impl ToString) -> Self {
        self.params.insert(key, value.to_string());
        self
    }
}

impl From<ParseError> for Problem {
    fn from(error: ParseError) -> Self {
        match error {
            ParseError::Invalid(error) => error.into(),
            ParseError::UnsupportedVersion(version) => {
                Self::new("disk_unsupported_version").with("version", version)
            }
            ParseError::UnsupportedKind(kind) => {
                Self::new("disk_unsupported_kind").with("kind", kind)
            }
        }
    }
}

impl From<ConfigError> for Problem {
    fn from(error: ConfigError) -> Self {
        let problem = Self::new(error.code()).with("detail", &error);
        match error {
            ConfigError::MissingField(field) => problem.with("field", field),
            ConfigError::WrongType { field, expected } => {
                problem.with("field", field).with("type", expected)
            }
            ConfigError::InvalidValue { field, reason } => {
                problem.with("field", field).with("reason", reason.code())
            }
            _ => problem,
        }
    }
}

impl From<WriteError> for Problem {
    fn from(error: WriteError) -> Self {
        Self::new(error.code()).with("detail", error)
    }
}

impl From<trust::TrustError> for Problem {
    fn from(error: trust::TrustError) -> Self {
        Self::new(error.code()).with("detail", error)
    }
}

impl ConfigForm {
    /// The configuration the form describes, checked like one read from a
    /// disk.
    fn to_config(&self) -> Result<DiskConfig, Problem> {
        let target = match self.kind.as_str() {
            steam::KIND => GameTarget::Steam(SteamGame {
                app_id: self.app_id.unwrap_or(0),
            }),
            exe::KIND => GameTarget::Exe(ExeGame {
                path: PathBuf::from(self.path.trim()),
                args: exe::split_args(&self.args),
            }),
            other => return Err(Problem::new("disk_unsupported_kind").with("kind", other)),
        };
        let process = Some(self.process.trim())
            .filter(|process| !process.is_empty())
            .map(str::to_owned);
        let config = DiskConfig {
            name: self.name.trim().to_owned(),
            target,
            process,
        };
        Ok(config.validated()?)
    }
}

/// The installed Steam games.
#[tauri::command]
pub async fn steam_games() -> Vec<LibraryGame> {
    blocking(steam::installed_games).await
}

/// Programs with an open window, read only now that the user asked.
#[tauri::command]
pub async fn open_applications() -> Vec<String> {
    blocking(launcher::windowed_programs).await
}

/// Asks the user for a program; `None` if they cancel.
#[tauri::command]
pub async fn pick_program(app: AppHandle) -> Option<String> {
    blocking(move || {
        let mut dialog = app.dialog().file().add_filter("Programs", &["exe"]);
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&window);
        }
        let path = dialog.blocking_pick_file()?.into_path().ok()?;
        Some(path.display().to_string())
    })
    .await
}

/// The file the form would write, or why it can't be written.
#[tauri::command]
pub fn config_preview(form: ConfigForm) -> Result<String, Problem> {
    Ok(form.to_config()?.to_toml())
}

/// Writes the form to the disk in the drive. A program written this way is
/// approved on this PC.
#[tauri::command]
pub async fn write_config(watcher: State<'_, Watcher>, form: ConfigForm) -> Result<(), Problem> {
    let config = form.to_config()?;
    let watcher = watcher.inner().clone();
    blocking(move || {
        let drive = writable_drive(&watcher)?;
        let hash = retry_in_use(|| diskconfig::write_drive(drive, &config))?;
        let approval = match config.target {
            GameTarget::Exe(_) => trust::approve(&hash, &config).map(|_| ()),
            _ => Ok(()),
        };
        watcher.update_view(|view| {
            view.set_content(&DiskContent::Configured {
                config: config.clone(),
                hash,
            });
        });
        watcher.record(events::disk_written(drive, &config.name));
        Ok(approval?)
    })
    .await
}

/// Removes the configuration from the disk in the drive.
#[tauri::command]
pub async fn remove_config(watcher: State<'_, Watcher>) -> Result<(), Problem> {
    let watcher = watcher.inner().clone();
    blocking(move || {
        let drive = writable_drive(&watcher)?;
        retry_in_use(|| diskconfig::remove_drive(drive))?;
        watcher.update_view(|view| view.set_content(&DiskContent::NotConfigured));
        watcher.record(events::disk_config_removed(drive));
        Ok(())
    })
    .await
}

/// The approved programs.
#[tauri::command]
pub fn approvals() -> Result<Vec<TrustEntry>, Problem> {
    Ok(TrustStore::load()?.entries().to_vec())
}

/// Approves the program on the disk in the drive.
#[tauri::command]
pub fn approve_disk(watcher: State<'_, Watcher>) -> Result<(), Problem> {
    let (config, hash) = watcher
        .view(|view| view.config().cloned())
        .ok_or_else(|| Problem::new("disk_none"))?;
    trust::approve(&hash, &config)?;
    watcher.update_view(|view| view.refresh_approval());
    watcher.record(events::trust_approved(&config.name));
    Ok(())
}

/// Revokes an approval.
#[tauri::command]
pub fn revoke_approval(watcher: State<'_, Watcher>, hash: String) -> Result<(), Problem> {
    let mut store = TrustStore::load()?;
    let entry = store.revoke(&hash).map_err(|error| match error {
        RevokeError::NotFound | RevokeError::Ambiguous(_) => Problem::new("trust_not_found"),
    })?;
    store.save()?;
    watcher.update_view(|view| view.refresh_approval());
    watcher.record(events::trust_revoked(&entry.name));
    Ok(())
}

/// The drive, if it has a disk the app has read: writing to a disk still
/// being read, or to an empty drive, makes no sense.
fn writable_drive(watcher: &Watcher) -> Result<floppy_station_core::floppy::DriveLetter, Problem> {
    if watcher.view(|view| view.has_disk()) {
        Ok(watcher.settings().drive)
    } else {
        Err(Problem::new("disk_none"))
    }
}

fn retry_in_use<T>(mut operation: impl FnMut() -> Result<T, WriteError>) -> Result<T, WriteError> {
    let mut tries = 1;
    loop {
        match operation() {
            Err(WriteError::InUse) if tries < IN_USE_TRIES => {
                tries += 1;
                thread::sleep(IN_USE_RETRY);
            }
            result => return result,
        }
    }
}

/// Runs disk or file work off the async runtime's threads.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .expect("blocking task panicked")
}
