//! Texts the backend shows itself (the tray menu and tooltip, notifications,
//! the file dialog), from the same language files as the frontend.

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

const EN_JSON: &str = include_str!("../../src/locales/en.json");
const PT_JSON: &str = include_str!("../../src/locales/pt.json");
const ES_JSON: &str = include_str!("../../src/locales/es.json");

type Texts = HashMap<String, String>;

fn parse(json: &str) -> Texts {
    serde_json::from_str(json).expect("language files are valid JSON")
}

static EN: LazyLock<Texts> = LazyLock::new(|| parse(EN_JSON));
static PT: LazyLock<Texts> = LazyLock::new(|| parse(PT_JSON));
static ES: LazyLock<Texts> = LazyLock::new(|| parse(ES_JSON));

/// The language in use: `en`, `pt` or `es`.
static CURRENT: RwLock<&str> = RwLock::new("en");

pub fn set_language(language: &'static str) {
    *CURRENT.write().unwrap_or_else(|error| error.into_inner()) = language;
}

pub fn language() -> &'static str {
    *CURRENT.read().unwrap_or_else(|error| error.into_inner())
}

fn texts(language: &str) -> &'static Texts {
    match language {
        "pt" => &PT,
        "es" => &ES,
        _ => &EN,
    }
}

/// The text for `key` in the current language (English if it has none),
/// with `{name}` placeholders filled from `params`. A missing key gives the
/// key itself.
pub fn t(key: &str, params: &[(&str, &str)]) -> String {
    let Some(template) = texts(language()).get(key).or_else(|| EN.get(key)) else {
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

    fn placeholders(text: &str) -> Vec<&str> {
        let mut names: Vec<&str> = text
            .split('{')
            .skip(1)
            .filter_map(|rest| rest.split('}').next())
            .collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn fills_placeholders() {
        assert_eq!(t("tray.no_disk", &[("drive", "A:")]), "A: no disk");
        assert_eq!(t("no.such.key", &[]), "no.such.key");
    }

    #[test]
    fn every_language_has_every_text_with_the_same_placeholders() {
        for (name, other) in [("pt", &*PT), ("es", &*ES)] {
            for (key, text) in EN.iter() {
                let translated = other
                    .get(key)
                    .unwrap_or_else(|| panic!("{name}.json has no {key}"));
                assert_eq!(
                    placeholders(translated),
                    placeholders(text),
                    "{name}.json: {key} has other placeholders"
                );
            }
            for key in other.keys() {
                assert!(
                    EN.contains_key(key),
                    "{name}.json has {key}, unknown in en.json"
                );
            }
        }
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
            "notify.untrusted",
            "notify.not_found",
            "notify.launch_failed",
            "notify.close_failed",
            "ui.dialog.programs",
        ];
        for key in keys {
            assert!(EN.contains_key(key), "en.json has no {key}");
        }
    }
}
