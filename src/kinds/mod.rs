//! Game kinds (the `kind` field of a disk configuration).
//!
//! Each kind lives in its own module with its own fields and validation. In
//! Phase 3 they also get launching and closing. Adding a kind means adding a
//! module and a variant to [`GameTarget`].

pub mod exe;
pub mod steam;

use std::fmt::Write as _;

use toml::Table;

use crate::diskconfig::ConfigError;

pub use exe::ExeGame;
pub use steam::SteamGame;

/// What a disk launches, with the fields specific to its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameTarget {
    Steam(SteamGame),
    Exe(ExeGame),
}

impl GameTarget {
    /// Value of the `kind` field.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Steam(_) => steam::KIND,
            Self::Exe(_) => exe::KIND,
        }
    }

    /// Reads the kind-specific fields out of the `[game]` table, removing the
    /// ones it uses. Returns `Ok(None)` for an unknown kind.
    pub(crate) fn from_fields(kind: &str, game: &mut Table) -> Result<Option<Self>, ConfigError> {
        Ok(Some(match kind {
            steam::KIND => Self::Steam(SteamGame::from_fields(game)?),
            exe::KIND => Self::Exe(ExeGame::from_fields(game)?),
            _ => return Ok(None),
        }))
    }

    /// Appends the kind-specific lines of the `[game]` table.
    pub(crate) fn write_fields(&self, out: &mut String) {
        match self {
            Self::Steam(game) => game.write_fields(out),
            Self::Exe(game) => game.write_fields(out),
        }
    }
}

/// Appends `key = value` to a TOML document being written.
pub(crate) fn write_line(out: &mut String, key: &str, value: impl std::fmt::Display) {
    let _ = writeln!(out, "{key} = {value}");
}
