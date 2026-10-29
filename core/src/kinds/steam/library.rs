//! The Steam games installed on this PC, read from Steam's own files: the
//! library folders listed in `steamapps\libraryfolders.vdf` and, in each,
//! one `appmanifest_<AppID>.acf` per game. Reads files only; Steam doesn't
//! need to be running.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::core::HSTRING;

use super::vdf;

/// An installed Steam game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryGame {
    pub app_id: u32,
    pub name: String,
}

/// Steam's own tools, installed like games but not playable.
const NOT_GAMES: &[u32] = &[
    228980, // Steamworks Common Redistributables
];

/// `StateFlags` bit for a fully installed app.
const FULLY_INSTALLED: u32 = 4;

/// The installed Steam games, sorted by name. Empty if Steam isn't
/// installed or its files can't be read.
pub fn installed_games() -> Vec<LibraryGame> {
    let Some(root) = steam_root() else {
        log::debug!("Steam not found");
        return Vec::new();
    };
    games_in(&library_folders(&root))
}

/// Where Steam is installed, from the registry.
fn steam_root() -> Option<PathBuf> {
    [
        (HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath"),
    ]
    .into_iter()
    .filter_map(|(key, subkey, value)| registry_string(key, subkey, value))
    .map(PathBuf::from)
    .find(|path| path.is_dir())
}

fn registry_string(key: HKEY, subkey: &str, value: &str) -> Option<String> {
    let (subkey, value) = (HSTRING::from(subkey), HSTRING::from(value));
    let mut buffer = [0u16; 1024];
    let mut size = size_of_val(&buffer) as u32;
    let result = unsafe {
        RegGetValueW(
            key,
            &subkey,
            &value,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    if result.is_err() {
        return None;
    }
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]))
}

/// The library folders: Steam's own folder plus those listed in
/// `libraryfolders.vdf`.
fn library_folders(root: &Path) -> Vec<PathBuf> {
    let mut folders = vec![root.to_path_buf()];
    let listed = fs::read_to_string(root.join(r"steamapps\libraryfolders.vdf"))
        .map(|text| vdf::parse(&text))
        .unwrap_or_default();
    if let Some(list) = listed.block("libraryfolders") {
        for (_, folder) in list.blocks() {
            if let Some(path) = folder.text("path") {
                folders.push(PathBuf::from(path));
            }
        }
    }
    // The same folder can appear with different case or slashes.
    let mut seen = Vec::new();
    folders.retain(|folder| {
        let key = folder.to_string_lossy().replace('/', "\\").to_lowercase();
        let key = key.trim_end_matches('\\').to_owned();
        let new = !seen.contains(&key);
        seen.push(key);
        new
    });
    folders
}

/// The fully installed games in the given library folders, sorted by name.
fn games_in(folders: &[PathBuf]) -> Vec<LibraryGame> {
    let mut games: Vec<LibraryGame> = Vec::new();
    for folder in folders {
        let Ok(entries) = fs::read_dir(folder.join("steamapps")) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Some(game) = fs::read_to_string(entry.path())
                .ok()
                .and_then(|text| from_manifest(&text))
            else {
                continue;
            };
            if !games.iter().any(|known| known.app_id == game.app_id) {
                games.push(game);
            }
        }
    }
    games.sort_by_cached_key(|game| game.name.to_lowercase());
    games
}

fn from_manifest(text: &str) -> Option<LibraryGame> {
    let file = vdf::parse(text);
    let app = file.block("AppState")?;
    let app_id: u32 = app.text("appid")?.parse().ok()?;
    let flags: u32 = app.text("StateFlags")?.parse().ok()?;
    let name = app.text("name")?.trim();
    if flags & FULLY_INSTALLED == 0 || name.is_empty() || NOT_GAMES.contains(&app_id) {
        return None;
    }
    Some(LibraryGame {
        app_id,
        name: name.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(app_id: u32, name: &str, flags: u32) -> String {
        format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\t\"{app_id}\"\n\t\"name\"\t\t\"{name}\"\n\
             \t\"StateFlags\"\t\t\"{flags}\"\n}}\n"
        )
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("floppy-station-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("steamapps")).unwrap();
        dir
    }

    #[test]
    fn reads_installed_games_only() {
        assert_eq!(
            from_manifest(&manifest(730, "Counter-Strike 2", 4)),
            Some(LibraryGame {
                app_id: 730,
                name: "Counter-Strike 2".into()
            })
        );
        // Downloading or update pending without a full install.
        assert_eq!(
            from_manifest(&manifest(730, "Counter-Strike 2", 1026)),
            None
        );
        assert_eq!(
            from_manifest(&manifest(228980, "Steamworks Common Redistributables", 4)),
            None
        );
        assert_eq!(from_manifest("garbage"), None);
    }

    #[test]
    fn lists_games_from_every_library_sorted_by_name() {
        let root = temp_dir("steam-root");
        let other = temp_dir("steam-library");
        fs::write(
            root.join(r"steamapps\libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\"0\"\n{{\n\"path\" \"{}\"\n}}\n\"1\"\n{{\n\"path\" \"{}\"\n}}\n}}\n",
                root.display().to_string().replace('\\', "\\\\"),
                other.display().to_string().replace('\\', "\\\\"),
            ),
        )
        .unwrap();
        fs::write(
            root.join(r"steamapps\appmanifest_286160.acf"),
            manifest(286160, "Tabletop Simulator", 4),
        )
        .unwrap();
        fs::write(
            other.join(r"steamapps\appmanifest_730.acf"),
            manifest(730, "Counter-Strike 2", 4),
        )
        .unwrap();
        fs::write(other.join(r"steamapps\notes.txt"), "not a manifest").unwrap();

        let folders = library_folders(&root);
        assert_eq!(folders.len(), 2, "the root is listed once: {folders:?}");
        let names: Vec<_> = games_in(&folders).into_iter().map(|g| g.name).collect();
        assert_eq!(names, ["Counter-Strike 2", "Tabletop Simulator"]);

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(other);
    }
}
