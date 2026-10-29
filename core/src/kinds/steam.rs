//! `kind = "steam"`: a Steam game, launched by its AppID.

mod library;
mod vdf;

pub use library::{LibraryGame, installed_games};

use toml::Table;
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
};
use windows::Win32::UI::Shell::{SE_ERR_NOASSOC, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

use super::write_line;
use crate::diskconfig::{ConfigError, InvalidReason, fields};
use crate::launcher::{LaunchError, Started};

pub const KIND: &str = "steam";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamGame {
    /// Steam AppID, as in `store.steampowered.com/app/<AppID>/`.
    pub app_id: u32,
}

impl SteamGame {
    pub(crate) fn from_fields(game: &mut Table) -> Result<Self, ConfigError> {
        const FIELD: &str = "steam_app_id";
        let value = fields::take_integer(game, FIELD)?.ok_or(ConfigError::MissingField(FIELD))?;
        let app_id =
            u32::try_from(value)
                .ok()
                .filter(|&id| id > 0)
                .ok_or(ConfigError::InvalidValue {
                    field: FIELD,
                    reason: InvalidReason::NotPositive,
                })?;
        Ok(Self { app_id })
    }

    /// Opens `steam://run/<AppID>`, which also starts Steam if it is closed.
    /// Does not wait for the game to open, and starts nothing the app can
    /// close by itself: closing relies on the config's `process`.
    pub(crate) fn launch(&self) -> Result<Started, LaunchError> {
        let url = HSTRING::from(format!("steam://run/{}", self.app_id));
        // The shell needs COM on the calling thread; initializing it again
        // is harmless.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        let result = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &url,
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        // Values above 32 mean success; lower ones are error codes.
        match result.0 as usize as u32 {
            code if code > 32 => Ok(Started::default()),
            code if code == SE_ERR_NOASSOC || code == ERROR_FILE_NOT_FOUND.0 => {
                Err(LaunchError::SteamMissing)
            }
            code => Err(LaunchError::Failed(code)),
        }
    }

    pub(crate) fn write_fields(&self, out: &mut String) {
        write_line(out, "steam_app_id", self.app_id);
    }
}
