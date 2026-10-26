//! Per-drive state machine ([`Poller`]) and the background polling thread
//! ([`Monitor`]) that feeds it.

use std::collections::BTreeMap;
use std::io;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::Win32::System::Diagnostics::Debug::{
    SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX, SetThreadErrorMode,
};

use super::probe::{MediaProbe, ProbeResult};
use super::{DriveLetter, DriveStatus, FloppyEvent, InsertOrigin, RemovalCause};

#[derive(Debug, Default)]
struct DriveTracker {
    /// `None` until the drive has been polled once.
    status: Option<DriveStatus>,
    /// Last known change token of the disk in the drive.
    change_token: Option<u64>,
    /// Last unexpected error, so it is only logged when it changes.
    last_error: Option<u32>,
}

/// Turns probe results into [`FloppyEvent`]s, keeping the state of every
/// watched drive. Has no thread of its own; [`Monitor`] runs it periodically.
pub struct Poller {
    probe: Box<dyn MediaProbe>,
    drives: BTreeMap<DriveLetter, DriveTracker>,
}

impl Poller {
    pub fn new(drives: impl IntoIterator<Item = DriveLetter>, probe: Box<dyn MediaProbe>) -> Self {
        Self {
            probe,
            drives: drives
                .into_iter()
                .map(|drive| (drive, DriveTracker::default()))
                .collect(),
        }
    }

    /// Current status of a drive; `None` if it is not watched or was not
    /// polled yet.
    pub fn status(&self, drive: DriveLetter) -> Option<&DriveStatus> {
        self.drives.get(&drive)?.status.as_ref()
    }

    /// Checks every watched drive once and returns what changed.
    pub fn poll(&mut self) -> Vec<FloppyEvent> {
        let mut events = Vec::new();
        for (&drive, tracker) in &mut self.drives {
            let result = self.probe.probe(drive);
            update(drive, tracker, result, self.probe.as_mut(), &mut events);
        }
        events
    }
}

fn update(
    drive: DriveLetter,
    tracker: &mut DriveTracker,
    result: ProbeResult,
    probe: &mut dyn MediaProbe,
    events: &mut Vec<FloppyEvent>,
) {
    let present_token = match result {
        ProbeResult::Unknown(code) => {
            if tracker.last_error != Some(code) {
                log::warn!("unexpected error {code} while checking drive {drive}; keeping state");
                tracker.last_error = Some(code);
            }
            return;
        }
        ProbeResult::Media { change_token } => Some(change_token),
        ProbeResult::DriveMissing | ProbeResult::NoMedia => None,
    };
    tracker.last_error = None;

    let mut insert = |tracker: &mut DriveTracker, origin, events: &mut Vec<FloppyEvent>| {
        let media = probe.read_media_info(drive);
        tracker.status = Some(DriveStatus::Present(media.clone()));
        events.push(FloppyEvent::MediaInserted { drive, media, origin });
    };

    let previous = tracker.status.take();
    match (previous, result) {
        // First poll: report the initial state.
        (None, ProbeResult::DriveMissing) => {
            tracker.status = Some(DriveStatus::Missing);
            events.push(FloppyEvent::DriveDisconnected { drive });
        }
        (None, ProbeResult::NoMedia) => {
            tracker.status = Some(DriveStatus::Empty);
            events.push(FloppyEvent::DriveConnected { drive });
        }
        (None, ProbeResult::Media { .. }) => {
            events.push(FloppyEvent::DriveConnected { drive });
            insert(tracker, InsertOrigin::PresentAtStartup, events);
        }

        (Some(DriveStatus::Missing), ProbeResult::NoMedia) => {
            tracker.status = Some(DriveStatus::Empty);
            events.push(FloppyEvent::DriveConnected { drive });
        }
        // Drive plugged back in with a disk inside: counts as an insertion.
        (Some(DriveStatus::Missing), ProbeResult::Media { .. }) => {
            events.push(FloppyEvent::DriveConnected { drive });
            insert(tracker, InsertOrigin::Inserted, events);
        }
        (Some(DriveStatus::Empty), ProbeResult::DriveMissing) => {
            tracker.status = Some(DriveStatus::Missing);
            events.push(FloppyEvent::DriveDisconnected { drive });
        }
        (Some(DriveStatus::Empty), ProbeResult::Media { .. }) => {
            insert(tracker, InsertOrigin::Inserted, events);
        }
        (Some(DriveStatus::Present(_)), ProbeResult::DriveMissing) => {
            tracker.status = Some(DriveStatus::Missing);
            events.push(FloppyEvent::MediaRemoved {
                drive,
                cause: RemovalCause::DriveDisconnected,
            });
            events.push(FloppyEvent::DriveDisconnected { drive });
        }
        (Some(DriveStatus::Present(_)), ProbeResult::NoMedia) => {
            tracker.status = Some(DriveStatus::Empty);
            events.push(FloppyEvent::MediaRemoved {
                drive,
                cause: RemovalCause::Ejected,
            });
        }
        (Some(DriveStatus::Present(media)), ProbeResult::Media { change_token }) => {
            let swapped = matches!(
                (tracker.change_token, change_token),
                (Some(old), Some(new)) if old != new
            );
            if swapped {
                // The disk was swapped between two polls.
                events.push(FloppyEvent::MediaRemoved {
                    drive,
                    cause: RemovalCause::Ejected,
                });
                insert(tracker, InsertOrigin::Inserted, events);
            } else {
                tracker.status = Some(DriveStatus::Present(media));
            }
        }
        // No change.
        (status, _) => tracker.status = status,
    }

    match present_token {
        Some(Some(token)) => tracker.change_token = Some(token),
        Some(None) => {}
        None => tracker.change_token = None,
    }
}

/// Background thread that polls the watched drives and sends
/// [`FloppyEvent`]s through a channel. Stops when dropped.
pub struct Monitor {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Monitor {
    /// Starts polling every `interval`. The thread ends when the monitor is
    /// dropped or when the receiving end of `events` is closed.
    pub fn spawn(
        mut poller: Poller,
        interval: Duration,
        events: Sender<FloppyEvent>,
    ) -> io::Result<Self> {
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let thread = thread::Builder::new()
            .name("floppy-monitor".into())
            .spawn(move || {
                suppress_error_dialogs();
                loop {
                    for event in poller.poll() {
                        if events.send(event).is_err() {
                            return;
                        }
                    }
                    // Waiting on the stop channel instead of sleeping lets
                    // the monitor stop immediately.
                    match stop_rx.recv_timeout(interval) {
                        Err(RecvTimeoutError::Timeout) => {}
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            })?;
        Ok(Self {
            stop: Some(stop_tx),
            thread: Some(thread),
        })
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Prevents Windows from showing "There is no disk in the drive" dialogs
/// when the polling thread touches an empty drive.
pub fn suppress_error_dialogs() {
    if let Err(error) =
        unsafe { SetThreadErrorMode(SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX, None) }
    {
        log::warn!("could not set thread error mode: {error}");
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::floppy::MediaInfo;

    const A: DriveLetter = DriveLetter('A');

    /// Probe that replays a scripted sequence of results. Each disk's
    /// serial is its change token, so tests can tell disks apart.
    struct FakeProbe {
        script: VecDeque<ProbeResult>,
        last: ProbeResult,
    }

    impl MediaProbe for FakeProbe {
        fn probe(&mut self, _drive: DriveLetter) -> ProbeResult {
            if let Some(next) = self.script.pop_front() {
                self.last = next;
            }
            self.last
        }

        fn read_media_info(&mut self, _drive: DriveLetter) -> MediaInfo {
            match self.last {
                ProbeResult::Media { change_token } => MediaInfo {
                    serial: change_token.map(|t| t as u32),
                    label: None,
                },
                _ => MediaInfo::default(),
            }
        }
    }

    fn disk(token: u64) -> ProbeResult {
        ProbeResult::Media {
            change_token: Some(token),
        }
    }

    fn media(token: u32) -> MediaInfo {
        MediaInfo {
            serial: Some(token),
            label: None,
        }
    }

    fn inserted(token: u32, origin: InsertOrigin) -> FloppyEvent {
        FloppyEvent::MediaInserted {
            drive: A,
            media: media(token),
            origin,
        }
    }

    fn removed(cause: RemovalCause) -> FloppyEvent {
        FloppyEvent::MediaRemoved { drive: A, cause }
    }

    const CONNECTED: FloppyEvent = FloppyEvent::DriveConnected { drive: A };
    const DISCONNECTED: FloppyEvent = FloppyEvent::DriveDisconnected { drive: A };

    /// Runs one poll per scripted result and returns the events of each.
    fn run(script: &[ProbeResult]) -> Vec<Vec<FloppyEvent>> {
        let probe = FakeProbe {
            script: script.iter().copied().collect(),
            last: ProbeResult::NoMedia,
        };
        let mut poller = Poller::new([A], Box::new(probe));
        script.iter().map(|_| poller.poll()).collect()
    }

    #[test]
    fn reports_initial_state() {
        assert_eq!(run(&[ProbeResult::DriveMissing]), [vec![DISCONNECTED]]);
        assert_eq!(run(&[ProbeResult::NoMedia]), [vec![CONNECTED]]);
        assert_eq!(
            run(&[disk(1)]),
            [vec![CONNECTED, inserted(1, InsertOrigin::PresentAtStartup)]]
        );
    }

    #[test]
    fn insert_and_remove() {
        let events = run(&[ProbeResult::NoMedia, disk(1), disk(1), ProbeResult::NoMedia]);
        assert_eq!(
            events,
            [
                vec![CONNECTED],
                vec![inserted(1, InsertOrigin::Inserted)],
                vec![],
                vec![removed(RemovalCause::Ejected)],
            ]
        );
    }

    #[test]
    fn detects_swap_between_polls() {
        let events = run(&[disk(1), disk(2)]);
        assert_eq!(
            events[1],
            [
                removed(RemovalCause::Ejected),
                inserted(2, InsertOrigin::Inserted)
            ]
        );
    }

    #[test]
    fn missing_change_token_is_not_a_swap() {
        let no_token = ProbeResult::Media { change_token: None };
        let events = run(&[disk(1), no_token, disk(1)]);
        assert_eq!(events[1], []);
        assert_eq!(events[2], []);
    }

    #[test]
    fn swap_seen_after_media_changed_error() {
        // ERROR_MEDIA_CHANGED yields no token; the next poll shows the new one.
        let no_token = ProbeResult::Media { change_token: None };
        let events = run(&[disk(1), no_token, disk(2)]);
        assert_eq!(events[1], []);
        assert_eq!(
            events[2],
            [
                removed(RemovalCause::Ejected),
                inserted(2, InsertOrigin::Inserted)
            ]
        );
    }

    #[test]
    fn drive_unplugged_with_disk() {
        let events = run(&[disk(1), ProbeResult::DriveMissing]);
        assert_eq!(
            events[1],
            [removed(RemovalCause::DriveDisconnected), DISCONNECTED]
        );
    }

    #[test]
    fn drive_replugged_with_disk_counts_as_insertion() {
        let events = run(&[ProbeResult::DriveMissing, disk(1)]);
        assert_eq!(
            events[1],
            [CONNECTED, inserted(1, InsertOrigin::Inserted)]
        );
    }

    #[test]
    fn empty_drive_unplugged_and_replugged() {
        let events = run(&[
            ProbeResult::NoMedia,
            ProbeResult::DriveMissing,
            ProbeResult::DriveMissing,
            ProbeResult::NoMedia,
        ]);
        assert_eq!(
            events,
            [vec![CONNECTED], vec![DISCONNECTED], vec![], vec![CONNECTED]]
        );
    }

    #[test]
    fn unknown_errors_keep_state() {
        let events = run(&[disk(1), ProbeResult::Unknown(5), disk(1), ProbeResult::NoMedia]);
        assert_eq!(events[1], []);
        assert_eq!(events[2], []);
        assert_eq!(events[3], [removed(RemovalCause::Ejected)]);
    }

    #[test]
    fn unknown_error_on_first_poll_reports_nothing_yet() {
        let events = run(&[ProbeResult::Unknown(5), ProbeResult::NoMedia]);
        assert_eq!(events, [vec![], vec![CONNECTED]]);
    }

    #[test]
    fn tracks_status() {
        let probe = FakeProbe {
            script: [disk(7)].into(),
            last: ProbeResult::NoMedia,
        };
        let mut poller = Poller::new([A], Box::new(probe));
        assert_eq!(poller.status(A), None);
        poller.poll();
        assert_eq!(poller.status(A), Some(&DriveStatus::Present(media(7))));
    }

    #[test]
    fn monitor_sends_events_and_stops_on_drop() {
        let probe = FakeProbe {
            script: [ProbeResult::NoMedia].into(),
            last: ProbeResult::NoMedia,
        };
        let (tx, rx) = mpsc::channel();
        let monitor =
            Monitor::spawn(Poller::new([A], Box::new(probe)), Duration::from_secs(60), tx)
                .unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), CONNECTED);
        drop(monitor); // must return promptly despite the 60 s interval
        assert!(rx.recv().is_err());
    }
}
