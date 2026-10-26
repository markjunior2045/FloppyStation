//! Console prototype entry point. Only wires the library modules together
//! and prints events; all logic lives in the `floppy_station` library.

use std::process::ExitCode;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use floppy_station::floppy::{DriveLetter, FloppyEvent, Monitor, Poller, ProbeKind, ProbeResult};
use floppy_station::settings::Settings;

const USAGE: &str = "\
usage: floppy-station [--drive <letter>] [--probe ioctl|volume] [--diag]

  --drive <letter>  drive to watch, overriding the settings file
  --probe <kind>    how to check the drive for a disk (default: ioctl)
  --diag            print the raw result and duration of every check";

struct Args {
    drive: Option<DriveLetter>,
    probe: ProbeKind,
    diag: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        drive: None,
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
            "--probe" => {
                let value = iter.next().ok_or("--probe needs a value")?;
                args.probe =
                    ProbeKind::parse(&value).ok_or(format!("unknown probe: {value}"))?;
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
    println!(
        "Watching drive {} every {} ms (probe: {}). Press Ctrl+C to quit.",
        settings.drive,
        settings.poll_interval_ms,
        args.probe.name()
    );

    if args.diag {
        run_diagnostics(&settings, args.probe);
        return ExitCode::SUCCESS;
    }

    let (tx, rx) = mpsc::channel();
    let poller = Poller::new([settings.drive], args.probe.create());
    let _monitor = match Monitor::spawn(poller, settings.poll_interval(), tx) {
        Ok(monitor) => monitor,
        Err(error) => {
            log::error!("could not start the drive monitor: {error}");
            return ExitCode::FAILURE;
        }
    };

    let start = Instant::now();
    for event in rx {
        println!("[{:>9.3}s] {}", start.elapsed().as_secs_f64(), describe(&event));
    }
    ExitCode::SUCCESS
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
