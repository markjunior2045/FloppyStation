//! `kind = "exe"`: any program, started from its executable. Shown as
//! "Program" in the UI.

use std::path::{Path, PathBuf};

use toml::Table;

use super::write_line;
use crate::diskconfig::{ConfigError, InvalidReason, fields};

pub const KIND: &str = "exe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExeGame {
    /// Absolute path of the executable. Specific to the PC the disk was
    /// configured on.
    pub path: PathBuf,
    /// Command-line arguments.
    pub args: Vec<String>,
}

impl ExeGame {
    /// Folder the program is started in: the executable's own folder, which
    /// many games require.
    pub fn working_dir(&self) -> Option<&Path> {
        self.path.parent()
    }

    pub(crate) fn from_fields(game: &mut Table) -> Result<Self, ConfigError> {
        let path = fields::take_string(game, "path")?.ok_or(ConfigError::MissingField("path"))?;
        let path = PathBuf::from(path.trim());
        if !path.is_absolute() {
            return Err(ConfigError::InvalidValue {
                field: "path",
                reason: InvalidReason::NotAbsolute,
            });
        }
        if !fields::has_exe_extension(&path) {
            return Err(ConfigError::InvalidValue {
                field: "path",
                reason: InvalidReason::NotExe,
            });
        }
        let args = fields::take_string_array(game, "args")?.unwrap_or_default();
        Ok(Self { path, args })
    }

    pub(crate) fn write_fields(&self, out: &mut String) {
        write_line(
            out,
            "path",
            fields::quote_path(&self.path.to_string_lossy()),
        );
        if !self.args.is_empty() {
            write_line(out, "args", fields::quote_array(&self.args));
        }
    }
}
