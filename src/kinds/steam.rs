//! `kind = "steam"`: a Steam game, launched by its AppID.

use toml::Table;

use super::write_line;
use crate::diskconfig::{ConfigError, InvalidReason, fields};

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

    pub(crate) fn write_fields(&self, out: &mut String) {
        write_line(out, "steam_app_id", self.app_id);
    }
}
