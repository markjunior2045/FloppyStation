//! Per-drive state machine ([`Poller`]) and the background polling thread
//! ([`Monitor`]) that feeds it.

use std::collections::BTreeMap;
use std::io;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::System::Diagnostics::Debug::{
    SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX, SetThreadErrorMode,
};

use super::probe::{MediaProbe, ProbeResult};
use super::wait::{DiskWaiter, PendingWait};
use super::{DriveLetter, DriveStatus, FloppyEvent, InsertOrigin, RemovalCause};

/// Wait before the second check that confirms a change. Floppy drives report
/// "not ready" once right after a disk change, and other programs touching
/// the drive can cause the same, so a single result is never trusted.
const DEFAULT_CONFIRM_DELAY: Duration = Duration::from_millis(250);

/// How often a pending wait for a disk is looked at. Touches no hardware.
const WAIT_CHECK: Duration = Duration::from_millis(250);

/// A wait for a disk that ends sooner than this, with the drive still
/// empty, is not working on this drive: the next one starts only after the
/// usual interval, so a failing wait doesn't turn into constant checks.
const MIN_WAIT: Duration = Duration::from_secs(2);

/// How often each drive is checked, depending on its state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollIntervals {
    /// With a disk in the drive (silent on floppy drives), or with the drive
    /// missing (no hardware access at all).
    pub present: Duration,
    /// With an empty drive. Floppy drives step their head to check for a
    /// disk, so each check makes an audible click.
    pub empty: Duration,
}

impl PollIntervals {
    fn for_status(&self, status: Option<&DriveStatus>) -> Duration {
        match status {
            Some(DriveStatus::Empty) => self.empty,
            _ => self.present,
        }
    }
}

struct DriveTracker {
    /// `None` until the drive has been polled once.
    status: Option<DriveStatus>,
    /// Last known change token of the disk in the drive.
    change_token: Option<u64>,
    /// Last unexpected error, so it is only logged when it changes.
    last_error: Option<u32>,
    /// When the drive should be checked next.
    next_due: Instant,
    /// Wait for a disk in progress (quiet detection, empty drive only).
    wait: Option<Wait>,
}

struct Wait {
    pending: Box<dyn PendingWait>,
    started: Instant,
}

/// Turns probe results into [`FloppyEvent`]s, keeping the state of every
/// watched drive. Has no thread of its own; [`Monitor`] runs it periodically.
pub struct Poller {
    probe: Box<dyn MediaProbe>,
    intervals: PollIntervals,
    confirm_delay: Duration,
    drives: BTreeMap<DriveLetter, DriveTracker>,
    /// Quiet detection: empty drives are waited on instead of polled.
    waiter: Option<Box<dyn DiskWaiter>>,
    quiet: bool,
}

impl Poller {
    /// Changes the time between checks. Takes effect from each drive's next
    /// check.
    pub fn set_intervals(&mut self, intervals: PollIntervals) {
        self.intervals = intervals;
    }

    pub fn new(
        drives: impl IntoIterator<Item = DriveLetter>,
        probe: Box<dyn MediaProbe>,
        intervals: PollIntervals,
    ) -> Self {
        let now = Instant::now();
        Self {
            probe,
            intervals,
            confirm_delay: DEFAULT_CONFIRM_DELAY,
            drives: drives
                .into_iter()
                .map(|drive| {
                    let tracker = DriveTracker {
                        status: None,
                        change_token: None,
                        last_error: None,
                        next_due: now,
                        wait: None,
                    };
                    (drive, tracker)
                })
                .collect(),
            waiter: None,
            quiet: false,
        }
    }

    /// Enables quiet detection through `waiter`: an empty drive is waited
    /// on (see [`DiskWaiter`]) instead of checked at the empty interval.
    /// `quiet` turns it on or off, like [`Poller::set_quiet`].
    pub fn with_waiter(mut self, waiter: Box<dyn DiskWaiter>, quiet: bool) -> Self {
        self.waiter = Some(waiter);
        self.quiet = quiet;
        self
    }

    /// Turns quiet detection on or off, from each drive's next check.
    /// Without a waiter, empty drives are always polled.
    pub fn set_quiet(&mut self, quiet: bool) {
        if quiet == self.quiet {
            return;
        }
        self.quiet = quiet;
        let now = Instant::now();
        for tracker in self.drives.values_mut() {
            tracker.wait = None;
            if matches!(tracker.status, Some(DriveStatus::Empty)) {
                tracker.next_due = now;
            }
        }
    }

    /// Changes the wait before a confirmation check (mainly for tests).
    pub fn with_confirm_delay(mut self, delay: Duration) -> Self {
        self.confirm_delay = delay;
        self
    }

    /// Current status of a drive; `None` if it is not watched or was not
    /// polled yet.
    pub fn status(&self, drive: DriveLetter) -> Option<&DriveStatus> {
        self.drives.get(&drive)?.status.as_ref()
    }

    /// Checks every watched drive once, regardless of its schedule.
    pub fn poll(&mut self) -> Vec<FloppyEvent> {
        self.poll_where(Instant::now(), |_| true)
    }

    /// Checks the drives whose next check is due at `now`.
    pub fn poll_due(&mut self, now: Instant) -> Vec<FloppyEvent> {
        self.poll_where(now, |tracker| tracker.next_due <= now)
    }

    /// When the next drive check is due; `None` if no drive is watched.
    pub fn next_due(&self) -> Option<Instant> {
        self.drives.values().map(|tracker| tracker.next_due).min()
    }

    fn poll_where(
        &mut self,
        now: Instant,
        due: impl Fn(&DriveTracker) -> bool,
    ) -> Vec<FloppyEvent> {
        let mut events = Vec::new();
        for (&drive, tracker) in &mut self.drives {
            if !due(tracker) {
                continue;
            }
            let mut waiter = match self.waiter.as_mut() {
                Some(waiter) if self.quiet => Some(waiter),
                _ => None,
            };
            let mut start_failed = false;
            if let Some(waiter) = waiter.as_mut().filter(|_| tracker.status.is_none())
                && let Some(pending) = waiter.resume(drive)
            {
                // An earlier monitor left a wait running: the drive is empty,
                // and checking it now would only queue behind that wait.
                tracker.status = Some(DriveStatus::Empty);
                events.push(FloppyEvent::DriveConnected { drive });
                tracker.wait = Some(Wait {
                    pending,
                    started: now,
                });
                tracker.next_due = now + WAIT_CHECK;
                continue;
            }
            let empty = matches!(tracker.status, Some(DriveStatus::Empty));
            if let Some(waiter) = waiter.filter(|_| empty) {
                match &mut tracker.wait {
                    Some(wait) => {
                        if !wait.pending.finished() {
                            tracker.next_due = now + WAIT_CHECK;
                            continue;
                        }
                        // Finished: check the drive below.
                    }
                    None => match waiter.start(drive) {
                        Ok(pending) => {
                            tracker.wait = Some(Wait {
                                pending,
                                started: now,
                            });
                            tracker.next_due = now + WAIT_CHECK;
                            continue;
                        }
                        Err(error) => {
                            log::warn!("could not wait for a disk in drive {drive}: {error}");
                            start_failed = true;
                        }
                    },
                }
            }

            let finished = tracker.wait.take();
            let result = probe_confirmed(self.probe.as_mut(), drive, tracker, self.confirm_delay);
            if let Some(result) = result {
                update(drive, tracker, result, &mut events);
            }
            let still_empty = matches!(tracker.status, Some(DriveStatus::Empty));
            let too_short =
                finished.is_some_and(|wait| now.saturating_duration_since(wait.started) < MIN_WAIT);
            tracker.next_due = if self.quiet && self.waiter.is_some() && still_empty {
                // Wait again right away, unless waiting doesn't work here.
                if start_failed || too_short {
                    now + self.intervals.empty
                } else {
                    now
                }
            } else {
                now + self.intervals.for_status(tracker.status.as_ref())
            };
        }
        events
    }
}

/// Probes the drive and, if the result means a change, probes again to
/// confirm it. Returns `None` when the change was not confirmed.
fn probe_confirmed(
    probe: &mut dyn MediaProbe,
    drive: DriveLetter,
    tracker: &DriveTracker,
    confirm_delay: Duration,
) -> Option<ProbeResult> {
    let first = probe.probe(drive);
    if !is_change(tracker, first) {
        return Some(first);
    }
    thread::sleep(confirm_delay);
    let second = probe.probe(drive);

    // On the very first poll, the first check may be the drive's one-off
    // "not ready" after a disk change, so the second one wins.
    if tracker.status.is_none() {
        return Some(second);
    }
    if same_kind(first, second) && is_change(tracker, second) {
        Some(second)
    } else {
        log::debug!("unconfirmed change on drive {drive} ({first:?} then {second:?}); ignored");
        None
    }
}

/// Whether `result` differs from what is known about the drive.
fn is_change(tracker: &DriveTracker, result: ProbeResult) -> bool {
    match (&tracker.status, result) {
        (_, ProbeResult::Unknown(_)) => false,
        (None, _) => true,
        (Some(DriveStatus::Missing), ProbeResult::DriveMissing) => false,
        (Some(DriveStatus::Empty), ProbeResult::NoMedia) => false,
        (Some(DriveStatus::Present), ProbeResult::Media { change_token }) => {
            matches!((tracker.change_token, change_token), (Some(old), Some(new)) if old != new)
        }
        _ => true,
    }
}

fn same_kind(a: ProbeResult, b: ProbeResult) -> bool {
    std::mem::discriminant(&a) == std::mem::discriminant(&b)
}

fn update(
    drive: DriveLetter,
    tracker: &mut DriveTracker,
    result: ProbeResult,
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

    let insert = |tracker: &mut DriveTracker, origin, events: &mut Vec<FloppyEvent>| {
        tracker.status = Some(DriveStatus::Present);
        events.push(FloppyEvent::MediaInserted { drive, origin });
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
            insert(tracker, InsertOrigin::DriveReconnected, events);
        }
        (Some(DriveStatus::Empty), ProbeResult::DriveMissing) => {
            tracker.status = Some(DriveStatus::Missing);
            events.push(FloppyEvent::DriveDisconnected { drive });
        }
        (Some(DriveStatus::Empty), ProbeResult::Media { .. }) => {
            insert(tracker, InsertOrigin::Inserted, events);
        }
        (Some(DriveStatus::Present), ProbeResult::DriveMissing) => {
            tracker.status = Some(DriveStatus::Missing);
            events.push(FloppyEvent::MediaRemoved {
                drive,
                cause: RemovalCause::DriveDisconnected,
            });
            events.push(FloppyEvent::DriveDisconnected { drive });
        }
        (Some(DriveStatus::Present), ProbeResult::NoMedia) => {
            tracker.status = Some(DriveStatus::Empty);
            events.push(FloppyEvent::MediaRemoved {
                drive,
                cause: RemovalCause::Ejected,
            });
        }
        (Some(DriveStatus::Present), ProbeResult::Media { change_token }) => {
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
                tracker.status = Some(DriveStatus::Present);
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
/// [`FloppyEvent`]s through a channel.
///
/// Dropping the monitor stops it without waiting for the thread: a check in
/// progress can block for a long time (about 20 s on a USB floppy drive
/// that was just plugged in), and it just finishes in the background.
pub struct Monitor {
    // Dropping this sender is the stop signal.
    commands: Sender<Command>,
}

enum Command {
    SetIntervals(PollIntervals),
    SetQuiet(bool),
}

impl Monitor {
    /// Starts polling, following the poller's per-drive schedule. The thread
    /// ends when the monitor is dropped or when the receiving end of
    /// `events` is closed.
    pub fn spawn(mut poller: Poller, events: Sender<FloppyEvent>) -> io::Result<Self> {
        let (commands_tx, commands_rx) = mpsc::channel();
        thread::Builder::new()
            .name("floppy-monitor".into())
            .spawn(move || {
                suppress_error_dialogs();
                loop {
                    for event in poller.poll_due(Instant::now()) {
                        if events.send(event).is_err() {
                            return;
                        }
                    }
                    let Some(next) = poller.next_due() else {
                        return;
                    };
                    // Waiting on the command channel instead of sleeping lets
                    // the monitor stop immediately.
                    let wait = next.saturating_duration_since(Instant::now());
                    match commands_rx.recv_timeout(wait) {
                        Err(RecvTimeoutError::Timeout) => {}
                        Ok(Command::SetIntervals(intervals)) => poller.set_intervals(intervals),
                        Ok(Command::SetQuiet(quiet)) => poller.set_quiet(quiet),
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            })?;
        Ok(Self {
            commands: commands_tx,
        })
    }

    /// Changes the time between checks without restarting, so the state of
    /// the drives (and the games launched from them) is kept.
    pub fn set_intervals(&self, intervals: PollIntervals) {
        let _ = self.commands.send(Command::SetIntervals(intervals));
    }

    /// Turns quiet detection on or off without restarting.
    pub fn set_quiet(&self, quiet: bool) {
        let _ = self.commands.send(Command::SetQuiet(quiet));
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
    const MISSING: ProbeResult = ProbeResult::DriveMissing;
    const EMPTY: ProbeResult = ProbeResult::NoMedia;
    const NO_TOKEN: ProbeResult = ProbeResult::Media { change_token: None };
    const CONNECTED: FloppyEvent = FloppyEvent::DriveConnected { drive: A };
    const DISCONNECTED: FloppyEvent = FloppyEvent::DriveDisconnected { drive: A };
    const INTERVALS: PollIntervals = PollIntervals {
        present: Duration::from_secs(1),
        empty: Duration::from_secs(3),
    };

    /// Probe that returns one scripted result per check, repeating the last
    /// one when the script runs out. Each disk's serial is its change token,
    /// so tests can tell disks apart.
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

    fn poller(script: &[ProbeResult]) -> Poller {
        let probe = FakeProbe {
            script: script.iter().copied().collect(),
            last: EMPTY,
        };
        Poller::new([A], Box::new(probe), INTERVALS).with_confirm_delay(Duration::ZERO)
    }

    /// Polls `count` times and returns the events of each poll.
    fn polls(script: &[ProbeResult], count: usize) -> Vec<Vec<FloppyEvent>> {
        let mut poller = poller(script);
        (0..count).map(|_| poller.poll()).collect()
    }

    fn disk(token: u64) -> ProbeResult {
        ProbeResult::Media {
            change_token: Some(token),
        }
    }

    fn inserted(origin: InsertOrigin) -> FloppyEvent {
        FloppyEvent::MediaInserted { drive: A, origin }
    }

    fn removed(cause: RemovalCause) -> FloppyEvent {
        FloppyEvent::MediaRemoved { drive: A, cause }
    }

    #[test]
    fn reports_initial_state() {
        assert_eq!(polls(&[MISSING], 1), [vec![DISCONNECTED]]);
        assert_eq!(polls(&[EMPTY], 1), [vec![CONNECTED]]);
        assert_eq!(
            polls(&[disk(1)], 1),
            [vec![CONNECTED, inserted(InsertOrigin::PresentAtStartup)]]
        );
    }

    #[test]
    fn disk_present_at_startup_despite_pending_media_change() {
        // The first check after a disk change reports "not ready".
        assert_eq!(
            polls(&[EMPTY, disk(1)], 1),
            [vec![CONNECTED, inserted(InsertOrigin::PresentAtStartup)]]
        );
    }

    #[test]
    fn insert_and_remove() {
        let script = [EMPTY, EMPTY, disk(1), disk(1), disk(1), EMPTY, EMPTY];
        assert_eq!(
            polls(&script, 4),
            [
                vec![CONNECTED],
                vec![inserted(InsertOrigin::Inserted)],
                vec![],
                vec![removed(RemovalCause::Ejected)],
            ]
        );
    }

    #[test]
    fn ignores_unconfirmed_removal() {
        // A single "not ready" while a disk is in is not a removal.
        let events = polls(&[disk(1), disk(1), EMPTY, disk(1), disk(1)], 3);
        assert_eq!(events[1], []);
        assert_eq!(events[2], []);
    }

    #[test]
    fn ignores_change_contradicted_by_confirmation() {
        let events = polls(&[EMPTY, EMPTY, disk(1), MISSING, EMPTY], 3);
        assert_eq!(events[1], []);
        assert_eq!(events[2], []);
    }

    #[test]
    fn detects_swap_between_polls() {
        let events = polls(&[disk(1), disk(1), disk(2), disk(2)], 2);
        assert_eq!(
            events[1],
            [
                removed(RemovalCause::Ejected),
                inserted(InsertOrigin::Inserted)
            ]
        );
    }

    #[test]
    fn missing_change_token_is_not_a_swap() {
        let events = polls(&[disk(1), disk(1), NO_TOKEN, disk(1)], 3);
        assert_eq!(events[1], []);
        assert_eq!(events[2], []);
    }

    #[test]
    fn swap_seen_after_media_changed_error() {
        // ERROR_MEDIA_CHANGED yields no token; the next poll shows the new one.
        let events = polls(&[disk(1), disk(1), NO_TOKEN, disk(2), disk(2)], 3);
        assert_eq!(events[1], []);
        assert_eq!(
            events[2],
            [
                removed(RemovalCause::Ejected),
                inserted(InsertOrigin::Inserted)
            ]
        );
    }

    #[test]
    fn drive_unplugged_with_disk() {
        let events = polls(&[disk(1), disk(1), MISSING, MISSING], 2);
        assert_eq!(
            events[1],
            [removed(RemovalCause::DriveDisconnected), DISCONNECTED]
        );
    }

    #[test]
    fn drive_replugged_with_disk_counts_as_insertion() {
        let events = polls(&[MISSING, MISSING, disk(1), disk(1)], 2);
        assert_eq!(
            events[1],
            [CONNECTED, inserted(InsertOrigin::DriveReconnected)]
        );
    }

    #[test]
    fn empty_drive_unplugged_and_replugged() {
        let script = [EMPTY, EMPTY, MISSING, MISSING, MISSING, EMPTY, EMPTY];
        assert_eq!(
            polls(&script, 4),
            [vec![CONNECTED], vec![DISCONNECTED], vec![], vec![CONNECTED]]
        );
    }

    #[test]
    fn unknown_errors_keep_state() {
        let script = [
            disk(1),
            disk(1),
            ProbeResult::Unknown(5),
            disk(1),
            EMPTY,
            EMPTY,
        ];
        let events = polls(&script, 4);
        assert_eq!(events[1], []);
        assert_eq!(events[2], []);
        assert_eq!(events[3], [removed(RemovalCause::Ejected)]);
    }

    #[test]
    fn unknown_error_on_first_poll_reports_nothing_yet() {
        let unknown = ProbeResult::Unknown(5);
        let events = polls(&[unknown, EMPTY, EMPTY], 2);
        assert_eq!(events, [vec![], vec![CONNECTED]]);
    }

    #[test]
    fn tracks_status() {
        let mut poller = poller(&[disk(7)]);
        assert_eq!(poller.status(A), None);
        poller.poll();
        assert!(matches!(poller.status(A), Some(DriveStatus::Present)));
    }

    #[test]
    fn empty_drive_is_polled_less_often() {
        // Checks the drive once and returns how long until the next check.
        fn next_wait(script: &[ProbeResult]) -> Duration {
            let mut poller = poller(script);
            let now = Instant::now();
            assert!(!poller.poll_due(now).is_empty(), "first poll must run");
            poller.next_due().unwrap() - now
        }

        assert_eq!(next_wait(&[EMPTY]), INTERVALS.empty);
        assert_eq!(next_wait(&[disk(1)]), INTERVALS.present);
        assert_eq!(next_wait(&[MISSING]), INTERVALS.present);

        let mut empty = poller(&[EMPTY]);
        let now = Instant::now();
        empty.poll_due(now);
        assert_eq!(empty.poll_due(now + INTERVALS.present), []);
        assert_eq!(empty.status(A), Some(&DriveStatus::Empty));
    }

    #[test]
    fn monitor_sends_events_and_stops_on_drop() {
        let intervals = PollIntervals {
            present: Duration::from_secs(60),
            empty: Duration::from_secs(60),
        };
        let probe = FakeProbe {
            script: [EMPTY].into(),
            last: EMPTY,
        };
        let poller = Poller::new([A], Box::new(probe), intervals);
        let (tx, rx) = mpsc::channel();
        let monitor = Monitor::spawn(poller, tx).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), CONNECTED);
        drop(monitor); // must return promptly despite the 60 s interval
        assert!(rx.recv().is_err());
    }

    /// Waiter controlled by the test through shared state.
    #[derive(Clone, Default)]
    struct FakeWaiter(std::sync::Arc<std::sync::Mutex<WaitState>>);

    #[derive(Default)]
    struct WaitState {
        starts: usize,
        alive: usize,
        finished: bool,
        fail: bool,
        /// A wait left running by an earlier poller.
        resumable: bool,
    }

    struct FakeWait(FakeWaiter);

    impl FakeWaiter {
        fn state(&self) -> std::sync::MutexGuard<'_, WaitState> {
            self.0.lock().unwrap()
        }
    }

    impl DiskWaiter for FakeWaiter {
        fn start(&mut self, _drive: DriveLetter) -> io::Result<Box<dyn PendingWait>> {
            let mut state = self.state();
            if state.fail {
                return Err(io::Error::other("no helper"));
            }
            state.starts += 1;
            state.alive += 1;
            state.finished = false;
            Ok(Box::new(FakeWait(self.clone())))
        }

        fn resume(&mut self, _drive: DriveLetter) -> Option<Box<dyn PendingWait>> {
            let mut state = self.state();
            if !std::mem::take(&mut state.resumable) {
                return None;
            }
            state.alive += 1;
            Some(Box::new(FakeWait(self.clone())))
        }
    }

    impl PendingWait for FakeWait {
        fn finished(&mut self) -> bool {
            self.0.state().finished
        }
    }

    impl Drop for FakeWait {
        fn drop(&mut self) {
            self.0.state().alive -= 1;
        }
    }

    fn quiet_poller(script: &[ProbeResult]) -> (Poller, FakeWaiter) {
        let waiter = FakeWaiter::default();
        let poller = poller(script).with_waiter(Box::new(waiter.clone()), true);
        (poller, waiter)
    }

    #[test]
    fn quiet_detection_waits_instead_of_polling() {
        // Any check while waiting would see the disk and report it early.
        let (mut poller, waiter) = quiet_poller(&[EMPTY, EMPTY, disk(1)]);
        assert_eq!(poller.poll(), [CONNECTED]);
        assert_eq!(poller.poll(), []);
        assert_eq!(poller.poll(), []);
        assert_eq!(waiter.state().starts, 1);

        waiter.state().finished = true;
        assert_eq!(poller.poll(), [inserted(InsertOrigin::Inserted)]);
        assert_eq!(waiter.state().alive, 0);
    }

    #[test]
    fn quiet_detection_waits_again_after_removal() {
        let (mut poller, waiter) = quiet_poller(&[EMPTY, EMPTY, disk(1), disk(1), EMPTY]);
        poller.poll();
        poller.poll();
        waiter.state().finished = true;
        assert_eq!(poller.poll(), [inserted(InsertOrigin::Inserted)]);
        assert_eq!(poller.poll(), [removed(RemovalCause::Ejected)]);
        assert_eq!(poller.poll(), []);
        assert_eq!(waiter.state().starts, 2);
        assert_eq!(waiter.state().alive, 1);
    }

    #[test]
    fn quiet_detection_waits_again_when_windows_gives_up() {
        let (mut poller, waiter) = quiet_poller(&[EMPTY]);
        let start = Instant::now();
        poller.poll_due(start);
        poller.poll_due(start);
        waiter.state().finished = true;
        // A long wait that ended without a disk: wait again at once.
        let later = start + Duration::from_secs(200);
        assert_eq!(poller.poll_due(later), []);
        assert_eq!(poller.next_due(), Some(later));
        poller.poll_due(later);
        assert_eq!(waiter.state().starts, 2);
    }

    #[test]
    fn wait_ending_at_once_falls_back_to_the_empty_interval() {
        let (mut poller, waiter) = quiet_poller(&[EMPTY]);
        let start = Instant::now();
        poller.poll_due(start);
        poller.poll_due(start);
        waiter.state().finished = true;
        let check = start + WAIT_CHECK;
        poller.poll_due(check);
        assert_eq!(poller.next_due(), Some(check + INTERVALS.empty));
    }

    #[test]
    fn failing_waiter_falls_back_to_polling() {
        let (mut poller, waiter) = quiet_poller(&[EMPTY, EMPTY, EMPTY, disk(1)]);
        waiter.state().fail = true;
        let start = Instant::now();
        poller.poll_due(start);
        assert_eq!(poller.poll_due(start), []);
        assert_eq!(poller.next_due(), Some(start + INTERVALS.empty));
        assert_eq!(
            poller.poll_due(start + INTERVALS.empty),
            [inserted(InsertOrigin::Inserted)]
        );
    }

    #[test]
    fn turning_quiet_detection_off_abandons_the_wait() {
        let (mut poller, waiter) = quiet_poller(&[EMPTY, EMPTY, disk(1)]);
        poller.poll();
        poller.poll();
        assert_eq!(waiter.state().alive, 1);
        poller.set_quiet(false);
        assert_eq!(waiter.state().alive, 0);
        assert_eq!(poller.poll(), [inserted(InsertOrigin::Inserted)]);

        poller.set_quiet(true);
        assert_eq!(waiter.state().starts, 1, "a disk is in: no wait");
    }

    #[test]
    fn takes_over_a_wait_left_by_an_earlier_monitor() {
        // Checking the drive would report the disk as already in it, so a
        // check here would show the wait wasn't trusted.
        let (mut poller, waiter) = quiet_poller(&[disk(1)]);
        waiter.state().resumable = true;
        assert_eq!(poller.poll(), [CONNECTED]);
        assert_eq!(poller.poll(), []);
        assert_eq!(waiter.state().starts, 0);

        waiter.state().finished = true;
        assert_eq!(poller.poll(), [inserted(InsertOrigin::Inserted)]);
    }

    #[test]
    fn no_wait_with_a_disk_in_or_the_drive_missing() {
        let (mut poller, waiter) = quiet_poller(&[disk(1), disk(1), MISSING, MISSING]);
        for _ in 0..4 {
            poller.poll();
        }
        assert_eq!(waiter.state().starts, 0);
    }

    /// Probe whose checks take a long time, like a just-plugged drive.
    struct SlowProbe;

    impl MediaProbe for SlowProbe {
        fn probe(&mut self, _drive: DriveLetter) -> ProbeResult {
            thread::sleep(Duration::from_secs(2));
            EMPTY
        }
    }

    #[test]
    fn dropping_monitor_does_not_wait_for_a_slow_check() {
        let poller = Poller::new([A], Box::new(SlowProbe), INTERVALS);
        let (tx, _rx) = mpsc::channel();
        let monitor = Monitor::spawn(poller, tx).unwrap();
        thread::sleep(Duration::from_millis(100)); // let the check start
        let start = Instant::now();
        drop(monitor);
        assert!(start.elapsed() < Duration::from_millis(500));
    }
}
