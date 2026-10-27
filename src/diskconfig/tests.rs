use std::path::PathBuf;

use super::*;
use crate::kinds::{ExeGame, SteamGame};

const STEAM: &str = r#"
version = 1

[game]
name = "Counter-Strike 2"
kind = "steam"
steam_app_id = 730
process = "cs2.exe"
"#;

const EXE: &str = r#"
version = 1

[game]
name = "Doom"
kind = "exe"
path = 'C:\Games\Doom\doom.exe'
args = ["-fullscreen", "-skill 3"]
"#;

fn parse(text: &str) -> Result<DiskConfig, ParseError> {
    DiskConfig::parse(text.as_bytes())
}

fn invalid(text: &str) -> ConfigError {
    match parse(text) {
        Err(ParseError::Invalid(error)) => error,
        other => panic!("expected an invalid config, got {other:?}"),
    }
}

/// Replaces `from` with `to` in a sample config.
fn edit(sample: &str, from: &str, to: &str) -> String {
    assert!(sample.contains(from), "{from:?} not in sample");
    sample.replace(from, to)
}

/// A temporary folder, deleted when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("floppy-station-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn parses_steam_game() {
    let config = parse(STEAM).unwrap();
    assert_eq!(config.name, "Counter-Strike 2");
    assert_eq!(config.target, GameTarget::Steam(SteamGame { app_id: 730 }));
    assert_eq!(config.process.as_deref(), Some("cs2.exe"));
}

#[test]
fn parses_exe_game() {
    let config = parse(EXE).unwrap();
    let GameTarget::Exe(game) = &config.target else {
        panic!("expected exe, got {:?}", config.target);
    };
    assert_eq!(game.path, PathBuf::from(r"C:\Games\Doom\doom.exe"));
    assert_eq!(game.args, ["-fullscreen", "-skill 3"]);
    assert_eq!(game.working_dir(), Some(Path::new(r"C:\Games\Doom")));
    assert_eq!(config.process, None);
}

#[test]
fn accepts_utf8_bom() {
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend_from_slice(STEAM.as_bytes());
    assert!(DiskConfig::parse(&bytes).is_ok());
}

#[test]
fn tolerates_unknown_fields() {
    let text = edit(
        STEAM,
        "[game]",
        "color = \"red\"\n\n[game]\ncover = \"x.png\"",
    );
    assert_eq!(parse(&text).unwrap(), parse(STEAM).unwrap());
}

#[test]
fn empty_process_means_none() {
    let text = edit(STEAM, r#"process = "cs2.exe""#, r#"process = "  ""#);
    assert_eq!(parse(&text).unwrap().process, None);
}

#[test]
fn rejects_syntax_errors() {
    assert_eq!(invalid("version = ").code(), "config_syntax");
    assert_eq!(
        invalid(&edit(STEAM, "[game]", "[game")).code(),
        "config_syntax"
    );
}

#[test]
fn rejects_missing_fields() {
    for field in ["version", "name", "kind", "steam_app_id"] {
        let text: String = STEAM
            .lines()
            .filter(|line| !line.starts_with(&format!("{field} =")))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(invalid(&text), ConfigError::MissingField(field), "{field}");
    }
    assert_eq!(invalid("version = 1"), ConfigError::MissingField("game"));
    let no_path = edit(EXE, r"path = 'C:\Games\Doom\doom.exe'", "");
    assert_eq!(invalid(&no_path), ConfigError::MissingField("path"));
}

#[test]
fn rejects_wrong_types() {
    let cases = [
        (
            edit(STEAM, "steam_app_id = 730", r#"steam_app_id = "730""#),
            "steam_app_id",
        ),
        (
            edit(STEAM, r#"name = "Counter-Strike 2""#, "name = 2"),
            "name",
        ),
        (
            edit(EXE, r#"args = ["-fullscreen", "-skill 3"]"#, "args = [1]"),
            "args",
        ),
        (
            edit(
                EXE,
                r#"args = ["-fullscreen", "-skill 3"]"#,
                r#"args = "-x""#,
            ),
            "args",
        ),
        ("version = 1\ngame = 3".to_owned(), "game"),
    ];
    for (text, field) in cases {
        assert!(
            matches!(invalid(&text), ConfigError::WrongType { field: f, .. } if f == field),
            "{field}"
        );
    }
}

#[test]
fn rejects_invalid_values() {
    let cases = [
        (
            edit(STEAM, r#""Counter-Strike 2""#, r#""   ""#),
            "name",
            InvalidReason::Empty,
        ),
        (
            edit(
                STEAM,
                r#""Counter-Strike 2""#,
                &format!("{:?}", "x".repeat(101)),
            ),
            "name",
            InvalidReason::TooLong,
        ),
        (
            edit(STEAM, "= 730", "= 0"),
            "steam_app_id",
            InvalidReason::NotPositive,
        ),
        (
            edit(STEAM, "= 730", "= -5"),
            "steam_app_id",
            InvalidReason::NotPositive,
        ),
        (
            edit(STEAM, "= 730", "= 99999999999"),
            "steam_app_id",
            InvalidReason::NotPositive,
        ),
        (
            edit(STEAM, "version = 1", "version = 0"),
            "version",
            InvalidReason::OutOfRange,
        ),
        (
            edit(STEAM, r#""cs2.exe""#, r#"'C:\Games\cs2.exe'"#),
            "process",
            InvalidReason::HasDirectory,
        ),
        (
            edit(STEAM, r#""cs2.exe""#, r#""cs2""#),
            "process",
            InvalidReason::NotExe,
        ),
        (
            edit(EXE, r"'C:\Games\Doom\doom.exe'", r"'Doom\doom.exe'"),
            "path",
            InvalidReason::NotAbsolute,
        ),
        (
            edit(
                EXE,
                r"'C:\Games\Doom\doom.exe'",
                r"'C:\Games\Doom\doom.bat'",
            ),
            "path",
            InvalidReason::NotExe,
        ),
    ];
    for (text, field, reason) in cases {
        assert_eq!(
            invalid(&text),
            ConfigError::InvalidValue { field, reason },
            "{field}: {reason:?}"
        );
    }
}

#[test]
fn reports_newer_version_and_unknown_kind_separately() {
    assert_eq!(
        parse(&edit(STEAM, "version = 1", "version = 2")),
        Err(ParseError::UnsupportedVersion(2))
    );
    assert_eq!(
        parse(&edit(STEAM, r#"kind = "steam""#, r#"kind = "epic""#)),
        Err(ParseError::UnsupportedKind("epic".into()))
    );
}

#[test]
fn rejects_large_and_non_utf8_files() {
    let large = format!("{STEAM}\n# {}", "x".repeat(MAX_FILE_SIZE));
    assert_eq!(invalid(&large), ConfigError::TooLarge);
    assert_eq!(
        DiskConfig::parse(b"version = 1\xFF"),
        Err(ParseError::Invalid(ConfigError::NotUtf8))
    );
}

#[test]
fn serializes_and_parses_back() {
    let configs = [
        parse(STEAM).unwrap(),
        parse(EXE).unwrap(),
        DiskConfig {
            name: r#"Quotes "and" \backslashes\"#.into(),
            target: GameTarget::Exe(ExeGame {
                path: r"C:\It's\game.exe".into(),
                args: vec![],
            }),
            process: None,
        },
    ];
    for config in configs {
        let text = config.to_toml();
        assert_eq!(parse(&text).unwrap(), config, "{text}");
    }
}

#[test]
fn writes_readable_windows_paths() {
    let text = parse(EXE).unwrap().to_toml();
    assert!(text.contains(r"path = 'C:\Games\Doom\doom.exe'"), "{text}");
    assert!(text.starts_with("# Floppy Station"), "{text}");
}

#[test]
fn hash_is_of_exact_bytes() {
    let a = ConfigHash::of(STEAM.as_bytes());
    assert_eq!(a, ConfigHash::of(STEAM.as_bytes()));
    assert_ne!(a, ConfigHash::of(format!("{STEAM}\n").as_bytes()));
    assert_eq!(a.to_string().len(), 64);
    assert_eq!(
        ConfigHash::of(b"").to_string(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn inspects_folder() {
    let dir = TempDir::new("inspect");
    assert_eq!(inspect_dir(&dir.0), DiskContent::NotConfigured);

    fs::write(dir.0.join(FILE_NAME), STEAM).unwrap();
    let DiskContent::Configured { config, hash } = inspect_dir(&dir.0) else {
        panic!("expected configured");
    };
    assert_eq!(config, parse(STEAM).unwrap());
    assert_eq!(hash, ConfigHash::of(STEAM.as_bytes()));

    assert_eq!(inspect_dir(&dir.0.join("missing")), DiskContent::NoDisk);

    fs::write(dir.0.join(FILE_NAME), "version = ").unwrap();
    assert_eq!(inspect_dir(&dir.0).code(), "disk_config_invalid");
    fs::write(
        dir.0.join(FILE_NAME),
        edit(STEAM, "version = 1", "version = 9"),
    )
    .unwrap();
    assert_eq!(inspect_dir(&dir.0), DiskContent::UnsupportedVersion(9));
}

#[test]
fn writes_without_touching_other_files() {
    let dir = TempDir::new("write");
    fs::write(dir.0.join("savegame.dat"), "keep me").unwrap();
    // Leftover from an interrupted write.
    fs::write(dir.0.join(TEMP_FILE_NAME), "garbage").unwrap();

    let config = parse(EXE).unwrap();
    let hash = write_dir(&dir.0, &config).unwrap();
    assert_eq!(
        inspect_dir(&dir.0),
        DiskContent::Configured {
            config: config.clone(),
            hash
        }
    );

    // Overwriting an existing configuration.
    let other = parse(STEAM).unwrap();
    write_dir(&dir.0, &other).unwrap();
    assert!(
        matches!(inspect_dir(&dir.0), DiskContent::Configured { config, .. } if config == other)
    );

    assert_eq!(
        fs::read_to_string(dir.0.join("savegame.dat")).unwrap(),
        "keep me"
    );
    assert!(!dir.0.join(TEMP_FILE_NAME).exists());
}

#[test]
fn removes_only_its_files() {
    let dir = TempDir::new("remove");
    fs::write(dir.0.join("savegame.dat"), "keep me").unwrap();
    assert!(!remove_dir(&dir.0).unwrap());

    write_dir(&dir.0, &parse(STEAM).unwrap()).unwrap();
    fs::write(dir.0.join(TEMP_FILE_NAME), "garbage").unwrap();
    assert!(remove_dir(&dir.0).unwrap());

    assert_eq!(inspect_dir(&dir.0), DiskContent::NotConfigured);
    assert!(!dir.0.join(TEMP_FILE_NAME).exists());
    assert!(dir.0.join("savegame.dat").exists());
}
