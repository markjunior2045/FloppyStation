//! The rules for launching and closing games: turns drive events into
//! actions and reports what happened as [`GameEvent`]s.
//!
//! - A disk inserted while the app runs (or in a drive that was plugged back
//!   in) launches its game. A disk already in the drive when the app starts
//!   does not.
//! - `kind = "exe"` disks only launch if their configuration was approved.
//! - Removing a disk closes only the game launched from it, as the settings
//!   say. Games the app did not launch are never closed.
//! - Several games may run at once, one per drive.

use std::collections::BTreeMap;
use std::sync::mpsc::Sender;
use std::thread;

use crate::diskconfig::{self, ConfigHash, DiskConfig, DiskContent, DiskInspection};
use crate::floppy::{DriveLetter, FloppyEvent, InsertOrigin, RemovalCause};
use crate::kinds::GameTarget;
use crate::launcher::{CloseMode, CloseOutcome, LaunchError, RunningGame};
use crate::settings::{DisconnectAction, RemovalAction, Settings};
use crate::trust;

/// What the controller did, for the console or the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEvent {
    /// A disk was inserted and read.
    DiskRead {
        drive: DriveLetter,
        inspection: DiskInspection,
    },
    /// The disk was already in the drive when the app started: its game is
    /// not launched.
    StartupDiskIgnored {
        drive: DriveLetter,
        name: String,
    },
    /// A `kind = "exe"` disk whose configuration is not approved: nothing
    /// was run.
    Untrusted {
        drive: DriveLetter,
        name: String,
        hash: ConfigHash,
    },
    Launched {
        drive: DriveLetter,
        name: String,
    },
    LaunchFailed {
        drive: DriveLetter,
        name: String,
        error: LaunchError,
    },
    /// The disk was removed but its game is left open.
    LeftOpen {
        drive: DriveLetter,
        name: String,
        reason: LeftOpenReason,
    },
    /// The disk was removed and its game is being closed.
    Closing {
        drive: DriveLetter,
        name: String,
        mode: CloseMode,
    },
    Closed {
        drive: DriveLetter,
        name: String,
        outcome: CloseOutcome,
    },
}

impl GameEvent {
    /// Stable code for the event, to be translated by the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Self::DiskRead { .. } => "disk_read",
            Self::StartupDiskIgnored { .. } => "game_ignored_at_startup",
            Self::Untrusted { .. } => "launch_untrusted",
            Self::Launched { .. } => "game_launched",
            Self::LaunchFailed { .. } => "launch_failed",
            Self::LeftOpen { .. } => "game_left_open",
            Self::Closing { .. } => "game_closing",
            Self::Closed { .. } => "game_closed",
        }
    }
}

/// Why a game was left open when its disk was removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftOpenReason {
    /// The settings say not to close games.
    Setting,
    /// The drive was unplugged and the settings say to ignore it.
    DriveDisconnected,
    /// Nothing the app can find to close: a Steam game without `process`.
    NothingToClose,
}

impl LeftOpenReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::Setting => "left_open_setting",
            Self::DriveDisconnected => "left_open_drive_disconnected",
            Self::NothingToClose => "left_open_no_process",
        }
    }
}

/// What the controller needs from the system, so the rules can be tested
/// without drives or games.
pub trait Environment {
    type Game: Send + 'static;

    fn inspect(&mut self, drive: DriveLetter) -> DiskInspection;
    fn is_trusted(&mut self, hash: &ConfigHash) -> bool;
    fn launch(&mut self, config: &DiskConfig) -> Result<Self::Game, LaunchError>;
    /// Closes a game and reports [`GameEvent::Closed`] on `events`. Must not
    /// block: closing can take as long as the close timeout.
    fn close(
        &mut self,
        game: Self::Game,
        mode: CloseMode,
        drive: DriveLetter,
        name: String,
        events: &Sender<GameEvent>,
    );
}

/// The real system: real drives, the approved list and real processes.
pub struct System;

impl Environment for System {
    type Game = RunningGame;

    fn inspect(&mut self, drive: DriveLetter) -> DiskInspection {
        diskconfig::inspect_drive(drive)
    }

    fn is_trusted(&mut self, hash: &ConfigHash) -> bool {
        trust::is_approved(hash)
    }

    fn launch(&mut self, config: &DiskConfig) -> Result<RunningGame, LaunchError> {
        RunningGame::launch(config)
    }

    fn close(
        &mut self,
        game: RunningGame,
        mode: CloseMode,
        drive: DriveLetter,
        name: String,
        events: &Sender<GameEvent>,
    ) {
        let events = events.clone();
        let spawned = thread::Builder::new()
            .name(format!("close {drive}"))
            .spawn(move || {
                let outcome = game.close(mode);
                let _ = events.send(GameEvent::Closed {
                    drive,
                    name,
                    outcome,
                });
            });
        if let Err(error) = spawned {
            log::error!("could not start closing the game: {error}");
        }
    }
}

/// A game the controller launched.
struct Active<G> {
    name: String,
    game: G,
    /// Whether removing the disk can close it.
    closable: bool,
}

pub struct Controller<E: Environment> {
    env: E,
    on_removal: RemovalAction,
    close_mode: CloseMode,
    on_drive_disconnect: DisconnectAction,
    games: BTreeMap<DriveLetter, Active<E::Game>>,
    events: Sender<GameEvent>,
}

impl<E: Environment> Controller<E> {
    pub fn new(env: E, settings: &Settings, events: Sender<GameEvent>) -> Self {
        let close_mode = match settings.on_removal {
            RemovalAction::Force => CloseMode::Force,
            _ => CloseMode::Graceful(settings.close_timeout()),
        };
        Self {
            env,
            on_removal: settings.on_removal,
            close_mode,
            on_drive_disconnect: settings.on_drive_disconnect,
            games: BTreeMap::new(),
            events,
        }
    }

    pub fn handle(&mut self, event: &FloppyEvent) {
        match *event {
            FloppyEvent::MediaInserted { drive, origin, .. } => self.inserted(drive, origin),
            FloppyEvent::MediaRemoved { drive, cause } => self.removed(drive, cause),
            FloppyEvent::DriveConnected { .. } | FloppyEvent::DriveDisconnected { .. } => {}
        }
    }

    fn inserted(&mut self, drive: DriveLetter, origin: InsertOrigin) {
        if let Some(previous) = self.games.remove(&drive) {
            // Not expected: a removal always comes first.
            log::warn!("forgetting {:?} in {drive}", previous.name);
        }
        let inspection = self.env.inspect(drive);
        let configured = match &inspection.content {
            DiskContent::Configured { config, hash } => Some((config.clone(), *hash)),
            _ => None,
        };
        self.send(GameEvent::DiskRead { drive, inspection });
        let Some((config, hash)) = configured else {
            return;
        };
        let name = config.name.clone();

        if origin == InsertOrigin::PresentAtStartup {
            self.send(GameEvent::StartupDiskIgnored { drive, name });
            return;
        }
        let is_exe = matches!(config.target, GameTarget::Exe(_));
        if is_exe && !self.env.is_trusted(&hash) {
            self.send(GameEvent::Untrusted { drive, name, hash });
            return;
        }
        match self.env.launch(&config) {
            Ok(game) => {
                self.games.insert(
                    drive,
                    Active {
                        name: name.clone(),
                        game,
                        closable: is_exe || config.process.is_some(),
                    },
                );
                self.send(GameEvent::Launched { drive, name });
            }
            Err(error) => self.send(GameEvent::LaunchFailed { drive, name, error }),
        }
    }

    fn removed(&mut self, drive: DriveLetter, cause: RemovalCause) {
        let Some(active) = self.games.remove(&drive) else {
            return;
        };
        let name = active.name;
        let reason = if cause == RemovalCause::DriveDisconnected
            && self.on_drive_disconnect == DisconnectAction::Ignore
        {
            Some(LeftOpenReason::DriveDisconnected)
        } else if self.on_removal == RemovalAction::Leave {
            Some(LeftOpenReason::Setting)
        } else if !active.closable {
            Some(LeftOpenReason::NothingToClose)
        } else {
            None
        };
        if let Some(reason) = reason {
            self.send(GameEvent::LeftOpen {
                drive,
                name,
                reason,
            });
            return;
        }
        let mode = self.close_mode;
        self.send(GameEvent::Closing {
            drive,
            name: name.clone(),
            mode,
        });
        self.env.close(active.game, mode, drive, name, &self.events);
    }

    fn send(&self, event: GameEvent) {
        let _ = self.events.send(event);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    use super::*;
    use crate::floppy::MediaInfo;

    const STEAM: &str =
        "version = 1\n[game]\nname = \"Steam\"\nkind = \"steam\"\nsteam_app_id = 1\n";
    const STEAM_PROCESS: &str = "version = 1\n[game]\nname = \"Steam\"\nkind = \"steam\"\nsteam_app_id = 1\nprocess = \"game.exe\"\n";
    const EXE: &str =
        "version = 1\n[game]\nname = \"Exe\"\nkind = \"exe\"\npath = 'C:\\game.exe'\n";

    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Launch(String),
        Close(String, CloseMode),
    }

    #[derive(Default)]
    struct Fake {
        disks: HashMap<DriveLetter, DiskContent>,
        trusted: HashSet<ConfigHash>,
        fail_launch: bool,
        calls: Vec<Call>,
    }

    impl Environment for &mut Fake {
        type Game = String;

        fn inspect(&mut self, drive: DriveLetter) -> DiskInspection {
            DiskInspection {
                content: self
                    .disks
                    .get(&drive)
                    .cloned()
                    .unwrap_or(DiskContent::NoDisk),
                write_protected: Some(false),
            }
        }

        fn is_trusted(&mut self, hash: &ConfigHash) -> bool {
            self.trusted.contains(hash)
        }

        fn launch(&mut self, config: &DiskConfig) -> Result<String, LaunchError> {
            if self.fail_launch {
                return Err(LaunchError::NotFound);
            }
            self.calls.push(Call::Launch(config.name.clone()));
            Ok(config.name.clone())
        }

        fn close(
            &mut self,
            game: String,
            mode: CloseMode,
            _drive: DriveLetter,
            _name: String,
            _events: &Sender<GameEvent>,
        ) {
            self.calls.push(Call::Close(game, mode));
        }
    }

    fn a() -> DriveLetter {
        DriveLetter::parse("A").unwrap()
    }

    fn configured(text: &str) -> DiskContent {
        DiskContent::Configured {
            config: DiskConfig::parse(text.as_bytes()).unwrap(),
            hash: ConfigHash::of(text.as_bytes()),
        }
    }

    fn insert(drive: DriveLetter, origin: InsertOrigin) -> FloppyEvent {
        FloppyEvent::MediaInserted {
            drive,
            media: MediaInfo::default(),
            origin,
        }
    }

    fn remove(drive: DriveLetter, cause: RemovalCause) -> FloppyEvent {
        FloppyEvent::MediaRemoved { drive, cause }
    }

    /// Runs `events` through a controller and returns the codes of what it
    /// reported (without `disk_read`).
    fn run(fake: &mut Fake, settings: &Settings, events: &[FloppyEvent]) -> Vec<&'static str> {
        let (tx, rx): (_, Receiver<GameEvent>) = mpsc::channel();
        let mut controller = Controller::new(fake, settings, tx);
        for event in events {
            controller.handle(event);
        }
        drop(controller);
        rx.try_iter()
            .map(|event| event.code())
            .filter(|&code| code != "disk_read")
            .collect()
    }

    fn cycle() -> [FloppyEvent; 2] {
        [
            insert(a(), InsertOrigin::Inserted),
            remove(a(), RemovalCause::Ejected),
        ]
    }

    #[test]
    fn launches_on_insert_and_closes_on_removal() {
        let mut fake = Fake::default();
        fake.disks.insert(a(), configured(STEAM_PROCESS));
        let codes = run(&mut fake, &Settings::default(), &cycle());
        assert_eq!(codes, ["game_launched", "game_closing"]);
        assert_eq!(
            fake.calls,
            [
                Call::Launch("Steam".into()),
                Call::Close("Steam".into(), CloseMode::Graceful(Duration::from_secs(10))),
            ]
        );
    }

    #[test]
    fn ignores_disk_present_at_startup() {
        let mut fake = Fake::default();
        fake.disks.insert(a(), configured(STEAM_PROCESS));
        let events = [
            insert(a(), InsertOrigin::PresentAtStartup),
            remove(a(), RemovalCause::Ejected),
        ];
        let codes = run(&mut fake, &Settings::default(), &events);
        assert_eq!(codes, ["game_ignored_at_startup"]);
        assert!(fake.calls.is_empty());
    }

    #[test]
    fn plugged_back_drive_counts_as_insertion() {
        let mut fake = Fake::default();
        fake.disks.insert(a(), configured(STEAM_PROCESS));
        let events = [
            insert(a(), InsertOrigin::Inserted),
            remove(a(), RemovalCause::DriveDisconnected),
            insert(a(), InsertOrigin::Inserted),
        ];
        let codes = run(&mut fake, &Settings::default(), &events);
        assert_eq!(codes, ["game_launched", "game_closing", "game_launched"]);
    }

    #[test]
    fn runs_exe_only_when_trusted() {
        let mut fake = Fake::default();
        fake.disks.insert(a(), configured(EXE));
        let codes = run(&mut fake, &Settings::default(), &cycle());
        assert_eq!(codes, ["launch_untrusted"]);
        assert!(fake.calls.is_empty());

        fake.trusted.insert(ConfigHash::of(EXE.as_bytes()));
        let codes = run(&mut fake, &Settings::default(), &cycle());
        assert_eq!(codes, ["game_launched", "game_closing"]);
    }

    #[test]
    fn follows_removal_setting() {
        let mut fake = Fake::default();
        fake.disks.insert(a(), configured(STEAM_PROCESS));
        let force = Settings {
            on_removal: RemovalAction::Force,
            ..Settings::default()
        };
        run(&mut fake, &force, &cycle());
        assert_eq!(fake.calls[1], Call::Close("Steam".into(), CloseMode::Force));

        let leave = Settings {
            on_removal: RemovalAction::Leave,
            ..Settings::default()
        };
        fake.calls.clear();
        let codes = run(&mut fake, &leave, &cycle());
        assert_eq!(codes, ["game_launched", "game_left_open"]);
        assert_eq!(fake.calls, [Call::Launch("Steam".into())]);
    }

    #[test]
    fn follows_disconnect_setting() {
        let mut fake = Fake::default();
        fake.disks.insert(a(), configured(STEAM_PROCESS));
        let ignore = Settings {
            on_drive_disconnect: DisconnectAction::Ignore,
            ..Settings::default()
        };
        let events = [
            insert(a(), InsertOrigin::Inserted),
            remove(a(), RemovalCause::DriveDisconnected),
        ];
        let codes = run(&mut fake, &ignore, &events);
        assert_eq!(codes, ["game_launched", "game_left_open"]);
        // Ejecting still closes.
        let codes = run(&mut fake, &ignore, &cycle());
        assert_eq!(codes, ["game_launched", "game_closing"]);
    }

    #[test]
    fn steam_without_process_is_left_open() {
        let mut fake = Fake::default();
        fake.disks.insert(a(), configured(STEAM));
        let codes = run(&mut fake, &Settings::default(), &cycle());
        assert_eq!(codes, ["game_launched", "game_left_open"]);
    }

    #[test]
    fn failed_launch_closes_nothing() {
        let mut fake = Fake {
            fail_launch: true,
            ..Fake::default()
        };
        fake.disks.insert(a(), configured(STEAM_PROCESS));
        let codes = run(&mut fake, &Settings::default(), &cycle());
        assert_eq!(codes, ["launch_failed"]);
        assert!(fake.calls.is_empty());
    }

    #[test]
    fn unconfigured_disk_does_nothing() {
        let mut fake = Fake::default();
        fake.disks.insert(a(), DiskContent::NotConfigured);
        let codes = run(&mut fake, &Settings::default(), &cycle());
        assert!(codes.is_empty());
        assert!(fake.calls.is_empty());
    }

    #[test]
    fn closes_only_the_removed_disks_game() {
        let b = DriveLetter::parse("B").unwrap();
        let mut fake = Fake::default();
        fake.disks.insert(a(), configured(STEAM_PROCESS));
        fake.disks.insert(
            b,
            configured(&STEAM_PROCESS.replace("\"Steam\"", "\"Other\"")),
        );
        let events = [
            insert(a(), InsertOrigin::Inserted),
            insert(b, InsertOrigin::Inserted),
            remove(b, RemovalCause::Ejected),
        ];
        run(&mut fake, &Settings::default(), &events);
        assert_eq!(
            fake.calls.last(),
            Some(&Call::Close(
                "Other".into(),
                CloseMode::Graceful(Duration::from_secs(10))
            ))
        );
        assert_eq!(fake.calls.len(), 3);
    }
}
