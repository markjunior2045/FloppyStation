//! Texts the backend shows itself (the tray menu and tooltip), from the same
//! language file as the frontend.

use std::collections::HashMap;
use std::sync::LazyLock;

pub const EN_JSON: &str = include_str!("../../src/locales/en.json");

static EN: LazyLock<HashMap<String, String>> =
    LazyLock::new(|| serde_json::from_str(EN_JSON).expect("en.json is valid JSON"));

/// The text for `key`, with `{name}` placeholders filled from `params`. A
/// missing key gives the key itself.
pub fn t(key: &str, params: &[(&str, &str)]) -> String {
    let Some(template) = EN.get(key) else {
        log::warn!("missing translation: {key}");
        return key.to_owned();
    };
    params.iter().fold(template.clone(), |text, (name, value)| {
        text.replace(&format!("{{{name}}}"), value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_placeholders() {
        assert_eq!(t("tray.no_disk", &[("drive", "A:")]), "A: no disk");
        assert_eq!(t("no.such.key", &[]), "no.such.key");
    }

    #[test]
    fn has_the_backend_texts() {
        let keys = [
            "tray.tooltip",
            "tray.open",
            "tray.detect",
            "tray.exit",
            "box.reading",
            "box.blank",
            "box.untrusted",
            "box.missing",
            "box.failed",
            "box.unusable",
            "box.newer",
            "box.configure",
            "box.review",
            "box.details",
        ];
        for key in keys {
            assert!(EN.contains_key(key), "en.json has no {key}");
        }
    }
}
