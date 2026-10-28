//! Parsing, validation and writing of the `floppy-station.toml` text.

use toml::Table;

use super::{ConfigError, DiskConfig, InvalidReason, MAX_FILE_SIZE, ParseError, fields};
use crate::kinds::GameTarget;

pub const CURRENT_VERSION: i64 = 1;
const MAX_NAME_CHARS: usize = 100;
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

pub(super) fn parse(bytes: &[u8]) -> Result<DiskConfig, ParseError> {
    if bytes.len() > MAX_FILE_SIZE {
        return Err(ConfigError::TooLarge.into());
    }
    let bytes = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|_| ConfigError::NotUtf8)?;
    let mut root: Table =
        toml::from_str(text).map_err(|error| ConfigError::Syntax(error.to_string()))?;

    let version =
        fields::take_integer(&mut root, "version")?.ok_or(ConfigError::MissingField("version"))?;
    if version > CURRENT_VERSION {
        return Err(ParseError::UnsupportedVersion(version));
    }
    if version < 1 {
        return Err(ConfigError::InvalidValue {
            field: "version",
            reason: InvalidReason::OutOfRange,
        }
        .into());
    }

    let mut game = match root.remove("game") {
        Some(toml::Value::Table(game)) => game,
        Some(_) => {
            return Err(ConfigError::WrongType {
                field: "game",
                expected: "table",
            }
            .into());
        }
        None => return Err(ConfigError::MissingField("game").into()),
    };

    let name = fields::take_string(&mut game, "name")?
        .ok_or(ConfigError::MissingField("name"))?
        .trim()
        .to_owned();
    if name.is_empty() {
        return Err(invalid("name", InvalidReason::Empty));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(invalid("name", InvalidReason::TooLong));
    }

    // The kind comes first: a disk made by a newer version of the app may use
    // rules this version does not know.
    let kind = fields::take_string(&mut game, "kind")?.ok_or(ConfigError::MissingField("kind"))?;
    let target = GameTarget::from_fields(&kind, &mut game)?
        .ok_or_else(|| ParseError::UnsupportedKind(kind.clone()))?;

    let process = fields::take_string(&mut game, "process")?
        .map(|process| process.trim().to_owned())
        .filter(|process| !process.is_empty());
    if let Some(process) = &process {
        if process.contains(['\\', '/', ':']) {
            return Err(invalid("process", InvalidReason::HasDirectory));
        }
        if !fields::has_exe_extension(process.as_ref()) {
            return Err(invalid("process", InvalidReason::NotExe));
        }
    }

    for key in root.keys() {
        log::warn!("disk config: ignoring unknown field {key:?}");
    }
    for key in game.keys() {
        log::warn!("disk config: ignoring unknown field \"game.{key}\"");
    }

    Ok(DiskConfig {
        name,
        target,
        process,
    })
}

pub(super) fn to_toml(config: &DiskConfig) -> String {
    let mut out = String::from("# Floppy Station disk configuration.\n");
    out += &format!("version = {CURRENT_VERSION}\n\n[game]\n");
    crate::kinds::write_line(&mut out, "name", fields::quote(&config.name));
    crate::kinds::write_line(&mut out, "kind", fields::quote(config.target.kind()));
    config.target.write_fields(&mut out);
    if let Some(process) = &config.process {
        crate::kinds::write_line(&mut out, "process", fields::quote(process));
    }
    out
}

fn invalid(field: &'static str, reason: InvalidReason) -> ParseError {
    ConfigError::InvalidValue { field, reason }.into()
}
