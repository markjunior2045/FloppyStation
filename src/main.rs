//! Console prototype entry point. Only wires the library modules together
//! and prints events; all logic lives in the `floppy_station` library.

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use floppy_station::controller::{Controller, GameEvent, System};
use floppy_station::diskconfig::{self, DiskConfig, DiskContent, DiskInspection, ParseError};
use floppy_station::floppy::{DriveLetter, FloppyEvent, Monitor, Poller, ProbeKind, ProbeResult};
use floppy_station::kinds::GameTarget;
use floppy_station::launcher::{CloseMode, CloseOutcome};
use floppy_station::settings::{
    DisconnectAction, MAX_POLL_INTERVAL_MS, MIN_POLL_INTERVAL_MS, RemovalAction, Settings,
};
use floppy_station::trust::{self, RevokeError, TrustStore};

const USAGE: &str = "\
usage: floppy-station [options]             watch the drive, launch and close games
       floppy-station disk inspect [options]  show what the disk in the drive contains
       floppy-station disk write <file> [options]
                                              validate a local config file and write
                                              it to the disk
       floppy-station disk remove [options]   remove the config from the disk
       floppy-station trust [options]         approve the program on the disk in the drive
       floppy-station trust list              list the approved disk configs
       floppy-station trust revoke <hash>     remove an approval (the start of the hash
                                              is enough)

options:
  --drive <letter>       drive to watch, overriding the settings file
  --empty-interval <ms>  time between checks of an empty drive, overriding
                         the settings file
  --on-removal <action>  close | force | none, overriding the settings file
  --on-disconnect <action>
                         removal | ignore, overriding the settings file
  --probe <kind>         how to check the drive for a disk (default: ioctl)
  --diag                 print the raw result and duration of every check";

enum Command {
    Watch,
    DiskInspect,
    DiskWrite(PathBuf),
    DiskRemove,
    Trust,
    TrustList,
    TrustRevoke(String),
}

struct Args {
    command: Command,
    drive: Option<DriveLetter>,
    empty_interval_ms: Option<u64>,
    on_removal: Option<RemovalAction>,
    on_disconnect: Option<DisconnectAction>,
    probe: ProbeKind,
    diag: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        command: Command::Watch,
        drive: None,
        empty_interval_ms: None,
        on_removal: None,
        on_disconnect: None,
        probe: ProbeKind::default(),
        diag: false,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--diag" => args.diag = true,
            "--drive" => {
                let value = iter.next().ok_or("--drive needs a value")?;
                args.drive =
                    Some(DriveLetter::parse(&value).ok_or(format!("invalid drive: {value}"))?);
            }
            "--empty-interval" => {
                let value = iter.next().ok_or("--empty-interval needs a value")?;
                let ms: u64 = value
                    .parse()
                    .map_err(|_| format!("invalid interval: {value}"))?;
                args.empty_interval_ms = Some(ms.clamp(MIN_POLL_INTERVAL_MS, MAX_POLL_INTERVAL_MS));
            }
            "--on-removal" => {
                let value = iter.next().ok_or("--on-removal needs a value")?;
                args.on_removal =
                    Some(RemovalAction::parse(&value).ok_or(format!("unknown action: {value}"))?);
            }
            "--on-disconnect" => {
                let value = iter.next().ok_or("--on-disconnect needs a value")?;
                args.on_disconnect = Some(
                    DisconnectAction::parse(&value).ok_or(format!("unknown action: {value}"))?,
                );
            }
            "--probe" => {
                let value = iter.next().ok_or("--probe needs a value")?;
                args.probe = ProbeKind::parse(&value).ok_or(format!("unknown probe: {value}"))?;
            }
            "disk" => {
                args.command = match iter.next().as_deref() {
                    Some("inspect") => Command::DiskInspect,
                    Some("write") => {
                        Command::DiskWrite(iter.next().ok_or("disk write needs a file")?.into())
                    }
                    Some("remove") => Command::DiskRemove,
                    _ => return Err("disk needs inspect, write or remove".into()),
                };
            }
            "trust" => {
                args.command = match iter.next().as_deref() {
                    None => Command::Trust,
                    Some("list") => Command::TrustList,
                    Some("revoke") => {
                        Command::TrustRevoke(iter.next().ok_or("trust revoke needs a hash")?)
                    }
                    Some(other) => return Err(format!("unknown trust command: {other}")),
                };
            }
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(args)
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("{message}\n");
            }
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    let (mut settings, error) = Settings::load_or_default();
    if let Some(error) = error {
        log::warn!("using default settings ({}): {error}", error.code());
    }
    if let Some(drive) = args.drive {
        settings.drive = drive;
    }
    if let Some(ms) = args.empty_interval_ms {
        settings.empty_poll_interval_ms = ms;
    }
    if let Some(action) = args.on_removal {
        settings.on_removal = action;
    }
    if let Some(action) = args.on_disconnect {
        settings.on_drive_disconnect = action;
    }
    match &args.command {
        Command::Watch => {}
        Command::DiskInspect => return disk_inspect(settings.drive),
        Command::DiskWrite(file) => return disk_write(settings.drive, file),
        Command::DiskRemove => return disk_remove(settings.drive),
        Command::Trust => return trust_disk(settings.drive),
        Command::TrustList => return trust_list(),
        Command::TrustRevoke(prefix) => return trust_revoke(prefix),
    }

    println!(
        "Watching drive {} every {} ms, {} ms when empty (probe: {}). Press Ctrl+C to quit.",
        settings.drive,
        settings.poll_interval_ms,
        settings.empty_poll_interval_ms,
        args.probe.name()
    );

    if args.diag {
        run_diagnostics(&settings, args.probe);
        return ExitCode::SUCCESS;
    }

    println!(
        "On removal: {}. On drive unplugged: {}.",
        describe_removal(&settings),
        match settings.on_drive_disconnect {
            DisconnectAction::Removal => "same as removal",
            DisconnectAction::Ignore => "leave the game open",
        }
    );

    let (tx, rx) = mpsc::channel();
    let poller = Poller::new(
        [settings.drive],
        args.probe.create(),
        settings.poll_intervals(),
    );
    let _monitor = match Monitor::spawn(poller, tx) {
        Ok(monitor) => monitor,
        Err(error) => {
            log::error!("could not start the drive monitor: {error}");
            return ExitCode::FAILURE;
        }
    };

    // Game events also come from the threads that close games, so they are
    // printed by a thread of their own.
    let start = Instant::now();
    let (game_tx, game_rx) = mpsc::channel();
    thread::spawn(move || {
        for event in game_rx {
            println!(
                "[{:>9.3}s]   {}",
                start.elapsed().as_secs_f64(),
                describe_game_event(&event)
            );
        }
    });
    let mut controller = Controller::new(System, &settings, game_tx);
    for event in rx {
        println!(
            "[{:>9.3}s] {}",
            start.elapsed().as_secs_f64(),
            describe(&event)
        );
        controller.handle(&event);
    }
    ExitCode::SUCCESS
}

fn disk_inspect(drive: DriveLetter) -> ExitCode {
    let start = Instant::now();
    let inspection = diskconfig::inspect_drive(drive);
    println!("{}", describe_inspection(&inspection));
    let (ioctl, volume) = diskconfig::write_protection_sources(drive);
    println!(
        "write protection by source: ioctl={} volume_flag={}",
        yes_no(ioctl),
        yes_no(volume)
    );
    println!("took {:.1} s", start.elapsed().as_secs_f64());
    if let DiskContent::Configured { hash, .. } = &inspection.content {
        println!("hash: {hash}");
    }
    ExitCode::SUCCESS
}

fn disk_write(drive: DriveLetter, file: &PathBuf) -> ExitCode {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("could not read {}: {error}", file.display());
            return ExitCode::FAILURE;
        }
    };
    let config = match DiskConfig::parse(&bytes) {
        Ok(config) => config,
        Err(error) => {
            eprintln!(
                "{} is not a valid config: {}",
                file.display(),
                describe_parse_error(&error)
            );
            return ExitCode::FAILURE;
        }
    };
    println!("writing to {drive} {}", describe_config(&config));
    let start = Instant::now();
    match diskconfig::write_drive(drive, &config) {
        Ok(hash) => {
            println!(
                "written in {:.1} s, hash {hash}",
                start.elapsed().as_secs_f64()
            );
            // A program written by the app on this PC is approved.
            if matches!(config.target, GameTarget::Exe(_)) {
                match trust::approve(&hash, &config) {
                    Ok(_) => println!("approved on this PC"),
                    Err(error) => eprintln!("could not approve it ({}): {error}", error.code()),
                }
            }
            println!("{}", describe_inspection(&diskconfig::inspect_drive(drive)));
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("write failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn disk_remove(drive: DriveLetter) -> ExitCode {
    match diskconfig::remove_drive(drive) {
        Ok(true) => {
            println!("config removed from {drive}");
            ExitCode::SUCCESS
        }
        Ok(false) => {
            println!("{drive} has no config");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("remove failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn trust_disk(drive: DriveLetter) -> ExitCode {
    let inspection = diskconfig::inspect_drive(drive);
    let DiskContent::Configured { config, hash } = &inspection.content else {
        eprintln!("nothing to approve: {}", describe_inspection(&inspection));
        return ExitCode::FAILURE;
    };
    let GameTarget::Exe(game) = &config.target else {
        println!("{:?} is a Steam game: it needs no approval", config.name);
        return ExitCode::SUCCESS;
    };
    let mut store = match TrustStore::load() {
        Ok(store) => store,
        Err(error) => {
            eprintln!(
                "could not read the approved configs ({}): {error}",
                error.code()
            );
            return ExitCode::FAILURE;
        }
    };
    if store.is_trusted(hash) {
        println!("{:?} is already approved", config.name);
        return ExitCode::SUCCESS;
    }
    println!("The disk in {drive} wants to run:");
    println!("  name:      {}", config.name);
    println!("  program:   {}", game.path.display());
    println!("  arguments: {:?}", game.args);
    println!(
        "  closes:    {}",
        config.process.as_deref().unwrap_or("the program itself")
    );
    println!("  hash:      {hash}");
    if !game.path.is_file() {
        println!("  (the program does not exist on this PC)");
    }
    print!("Trust this disk? [y/N] ");
    let _ = io::stdout().flush();
    let mut answer = String::new();
    let _ = io::stdin().lock().read_line(&mut answer);
    if !matches!(answer.trim(), "y" | "Y" | "yes") {
        println!("not approved");
        return ExitCode::SUCCESS;
    }
    store.add(hash, config);
    match store.save() {
        Ok(()) => {
            println!("approved; remove and insert the disk to launch it");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("could not save the approval ({}): {error}", error.code());
            ExitCode::FAILURE
        }
    }
}

fn trust_list() -> ExitCode {
    let store = match TrustStore::load() {
        Ok(store) => store,
        Err(error) => {
            eprintln!(
                "could not read the approved configs ({}): {error}",
                error.code()
            );
            return ExitCode::FAILURE;
        }
    };
    if store.entries().is_empty() {
        println!("no approved configs");
    }
    for entry in store.entries() {
        println!(
            "{}  {}  {:?}  {}",
            &entry.hash[..12.min(entry.hash.len())],
            entry.added,
            entry.name,
            entry.path
        );
    }
    ExitCode::SUCCESS
}

fn trust_revoke(prefix: &str) -> ExitCode {
    let mut store = match TrustStore::load() {
        Ok(store) => store,
        Err(error) => {
            eprintln!(
                "could not read the approved configs ({}): {error}",
                error.code()
            );
            return ExitCode::FAILURE;
        }
    };
    let entry = match store.revoke(prefix) {
        Ok(entry) => entry,
        Err(RevokeError::NotFound) => {
            eprintln!("no approved config starts with {prefix:?}");
            return ExitCode::FAILURE;
        }
        Err(RevokeError::Ambiguous(count)) => {
            eprintln!("{count} approved configs start with {prefix:?}; give more of the hash");
            return ExitCode::FAILURE;
        }
    };
    match store.save() {
        Ok(()) => {
            println!("revoked {:?} ({})", entry.name, entry.path);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("could not save ({}): {error}", error.code());
            ExitCode::FAILURE
        }
    }
}

fn describe_removal(settings: &Settings) -> String {
    match settings.on_removal {
        RemovalAction::Close => format!(
            "ask the game to close, force it after {} s",
            settings.close_timeout_s
        ),
        RemovalAction::Force => "force the game to close".to_owned(),
        RemovalAction::Leave => "leave the game open".to_owned(),
    }
}

fn describe_game_event(event: &GameEvent) -> String {
    match event {
        GameEvent::DiskRead { inspection, .. } => describe_inspection(inspection),
        GameEvent::StartupDiskIgnored { name, .. } => {
            format!("{name:?} not launched: the disk was already in the drive at startup")
        }
        GameEvent::Untrusted { name, .. } => format!(
            "{name:?} not launched: this program is not approved (run `floppy-station trust`)"
        ),
        GameEvent::Launched { name, .. } => format!("launched {name:?}"),
        GameEvent::Reattached { name, .. } => {
            format!(
                "{name:?} is still open from before the drive was unplugged: not launched again"
            )
        }
        GameEvent::LaunchFailed { name, error, .. } => {
            format!("could not launch {name:?}: {error}")
        }
        GameEvent::LeftOpen { name, reason, .. } => {
            format!("left {name:?} open ({})", reason.code())
        }
        GameEvent::Closing { name, mode, .. } => match mode {
            CloseMode::Graceful(timeout) => format!(
                "closing {name:?} (asking first, forcing after {} s)",
                timeout.as_secs()
            ),
            CloseMode::Force => format!("forcing {name:?} to close"),
        },
        GameEvent::Closed { name, outcome, .. } => match outcome {
            CloseOutcome::AlreadyClosed => format!("{name:?} was already closed"),
            CloseOutcome::Closed => format!("{name:?} closed"),
            CloseOutcome::Forced => format!("{name:?} forced to close"),
            CloseOutcome::Failed { remaining } => format!(
                "could not close {name:?}: {remaining} process(es) still running ({})",
                outcome.code()
            ),
        },
    }
}

fn describe_inspection(inspection: &DiskInspection) -> String {
    let content = match &inspection.content {
        DiskContent::Configured { config, .. } => {
            format!("configured: {}", describe_config(config))
        }
        DiskContent::NotConfigured => "not configured".to_owned(),
        DiskContent::Invalid(error) => format!("invalid config ({}): {error}", error.code()),
        DiskContent::UnsupportedVersion(version) => {
            format!("config made by a newer version of the app (format version {version})")
        }
        DiskContent::UnsupportedKind(kind) => format!("unsupported kind {kind:?}"),
        DiskContent::Unformatted => "unformatted disk".to_owned(),
        DiskContent::NoDisk => return "no disk in the drive".to_owned(),
        DiskContent::Unreadable(code) => match code {
            Some(code) => format!("unreadable disk (error {code})"),
            None => "unreadable disk".to_owned(),
        },
    };
    format!(
        "{content}; write-protected: {}",
        yes_no(inspection.write_protected)
    )
}

fn describe_config(config: &DiskConfig) -> String {
    let target = match &config.target {
        GameTarget::Steam(game) => format!("steam {}", game.app_id),
        GameTarget::Exe(game) if game.args.is_empty() => format!("exe {}", game.path.display()),
        GameTarget::Exe(game) => format!("exe {} {:?}", game.path.display(), game.args),
    };
    let process = config.process.as_deref().unwrap_or("-");
    format!("{:?} ({target}), process {process}", config.name)
}

fn describe_parse_error(error: &ParseError) -> String {
    match error {
        ParseError::Invalid(error) => format!("{} ({})", error, error.code()),
        ParseError::UnsupportedVersion(version) => format!("unsupported format version {version}"),
        ParseError::UnsupportedKind(kind) => format!("unsupported kind {kind:?}"),
    }
}

fn yes_no(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}

fn describe(event: &FloppyEvent) -> String {
    let drive = event.drive();
    match event {
        FloppyEvent::DriveConnected { .. } => format!("drive {drive} connected"),
        FloppyEvent::DriveDisconnected { .. } => format!("drive {drive} disconnected"),
        FloppyEvent::MediaInserted { media, origin, .. } => {
            let serial = media
                .serial
                .map_or("none (unformatted?)".to_owned(), |s| format!("{s:08X}"));
            let label = media.label.as_deref().unwrap_or("-");
            format!("disk inserted in {drive} ({origin:?}) serial={serial} label={label}")
        }
        FloppyEvent::MediaRemoved { cause, .. } => {
            format!("disk removed from {drive} ({cause:?})")
        }
    }
}

/// Polls the drive directly, without the state machine, printing every raw
/// result and how long each check took.
fn run_diagnostics(settings: &Settings, kind: ProbeKind) {
    floppy_station::floppy::suppress_error_dialogs();
    let mut probe = kind.create();
    let start = Instant::now();
    let mut previous = None;
    loop {
        let begin = Instant::now();
        let result = probe.probe(settings.drive);
        let took = begin.elapsed();
        let mut line = format!(
            "[{:>9.3}s] {result:?} in {:.1} ms",
            start.elapsed().as_secs_f64(),
            took.as_secs_f64() * 1000.0
        );
        if matches!(result, ProbeResult::Media { .. }) && previous != Some(result) {
            let begin = Instant::now();
            let info = probe.read_media_info(settings.drive);
            line += &format!(
                " | {info:?} in {:.1} ms",
                begin.elapsed().as_secs_f64() * 1000.0
            );
        }
        println!("{line}");
        previous = Some(result);
        thread::sleep(settings.poll_interval());
    }
}
