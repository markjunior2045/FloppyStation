//! Helpers to read and write individual fields of a disk configuration.

use std::path::Path;

use toml::{Table, Value};

use super::ConfigError;

/// Removes `field` from `table`, requiring a string if present.
pub(crate) fn take_string(
    table: &mut Table,
    field: &'static str,
) -> Result<Option<String>, ConfigError> {
    match table.remove(field) {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(ConfigError::WrongType {
            field,
            expected: "string",
        }),
    }
}

/// Removes `field` from `table`, requiring an integer if present.
pub(crate) fn take_integer(
    table: &mut Table,
    field: &'static str,
) -> Result<Option<i64>, ConfigError> {
    match table.remove(field) {
        None => Ok(None),
        Some(Value::Integer(value)) => Ok(Some(value)),
        Some(_) => Err(ConfigError::WrongType {
            field,
            expected: "integer",
        }),
    }
}

/// Removes `field` from `table`, requiring an array of strings if present.
pub(crate) fn take_string_array(
    table: &mut Table,
    field: &'static str,
) -> Result<Option<Vec<String>>, ConfigError> {
    let wrong_type = ConfigError::WrongType {
        field,
        expected: "array of strings",
    };
    match table.remove(field) {
        None => Ok(None),
        Some(Value::Array(items)) => items
            .into_iter()
            .map(|item| match item {
                Value::String(text) => Ok(text),
                _ => Err(wrong_type.clone()),
            })
            .collect::<Result<_, _>>()
            .map(Some),
        Some(_) => Err(wrong_type),
    }
}

pub(crate) fn has_exe_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

/// A TOML basic string (`"..."`), with escapes as needed.
pub(crate) fn quote(text: &str) -> String {
    Value::String(text.to_owned()).to_string()
}

/// A TOML literal string (`'...'`) when possible, so Windows paths keep
/// their backslashes unescaped and stay readable.
pub(crate) fn quote_path(text: &str) -> String {
    if text.contains('\'') || text.chars().any(char::is_control) {
        quote(text)
    } else {
        format!("'{text}'")
    }
}

pub(crate) fn quote_array(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|item| quote(item)).collect();
    format!("[{}]", items.join(", "))
}
