//! The current state of the drive in one line, for the tray, kept up to date
//! from the events.

use floppy_station_core::controller::GameEvent;
use floppy_station_core::diskconfig::DiskContent;
use floppy_station_core::floppy::{DriveLetter, FloppyEvent};

use crate::locale::t;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Disk {
    #[default]
    None,
    Reading,
    Configured(String),
    NotConfigured,
    /// Damaged, unformatted or from a newer version.
    Unusable,
}

#[derive(Debug)]
pub struct DriveView {
    drive: DriveLetter,
    detection: bool,
    /// `None` until the drive is first checked.
    connected: Option<bool>,
    disk: Disk,
    /// Game launched from the disk in the drive.
    launched: Option<String>,
}

impl DriveView {
    pub fn new(drive: DriveLetter, detection: bool) -> Self {
        Self {
            drive,
            detection,
            connected: None,
            disk: Disk::None,
            launched: None,
        }
    }

    pub fn set_detection(&mut self, enabled: bool) {
        *self = Self::new(self.drive, enabled);
    }

    pub fn on_floppy(&mut self, event: &FloppyEvent) {
        match event {
            FloppyEvent::DriveConnected { .. } => self.connected = Some(true),
            FloppyEvent::DriveDisconnected { .. } => {
                self.connected = Some(false);
                self.disk = Disk::None;
                self.launched = None;
            }
            FloppyEvent::MediaInserted { .. } => self.disk = Disk::Reading,
            FloppyEvent::MediaRemoved { .. } => {
                self.disk = Disk::None;
                self.launched = None;
            }
        }
    }

    pub fn on_game(&mut self, event: &GameEvent) {
        match event {
            GameEvent::DiskRead { inspection, .. } => {
                self.disk = match &inspection.content {
                    DiskContent::Configured { config, .. } => Disk::Configured(config.name.clone()),
                    DiskContent::NotConfigured => Disk::NotConfigured,
                    DiskContent::NoDisk => Disk::None,
                    _ => Disk::Unusable,
                }
            }
            GameEvent::Launched { name, .. } | GameEvent::Reattached { name, .. } => {
                self.launched = Some(name.clone());
            }
            _ => {}
        }
    }

    /// The state in one line, e.g. `A: Counter-Strike 2 (launched)`.
    pub fn line(&self) -> String {
        let drive = self.drive.to_string();
        let with_drive = |key| t(key, &[("drive", &drive)]);
        if !self.detection {
            return t("tray.detection_off", &[]);
        }
        match self.connected {
            None => return t("tray.starting", &[]),
            Some(false) => return with_drive("tray.drive_missing"),
            Some(true) => {}
        }
        if let Some(name) = &self.launched {
            return t("tray.launched", &[("drive", &drive), ("name", name)]);
        }
        match &self.disk {
            Disk::None => with_drive("tray.no_disk"),
            Disk::Reading => with_drive("tray.reading"),
            Disk::Configured(name) => t("tray.configured", &[("drive", &drive), ("name", name)]),
            Disk::NotConfigured => with_drive("tray.not_configured"),
            Disk::Unusable => with_drive("tray.unusable"),
        }
    }
}

#[cfg(test)]
mod tests {
    use floppy_station_core::diskconfig::{ConfigHash, DiskConfig, DiskInspection};
    use floppy_station_core::floppy::{InsertOrigin, MediaInfo, RemovalCause};

    use super::*;

    fn a() -> DriveLetter {
        DriveLetter::parse("A").unwrap()
    }

    fn read(content: DiskContent) -> GameEvent {
        GameEvent::DiskRead {
            drive: a(),
            inspection: DiskInspection {
                content,
                write_protected: Some(false),
                volume: MediaInfo::default(),
            },
        }
    }

    #[test]
    fn follows_a_disk_from_insertion_to_removal() {
        let mut view = DriveView::new(a(), true);
        assert_eq!(view.line(), "Starting...");
        view.on_floppy(&FloppyEvent::DriveConnected { drive: a() });
        assert_eq!(view.line(), "A: no disk");
        view.on_floppy(&FloppyEvent::MediaInserted {
            drive: a(),
            origin: InsertOrigin::Inserted,
        });
        assert_eq!(view.line(), "A: reading disk...");
        let text = b"version = 1\n[game]\nname = \"Doom\"\nkind = \"steam\"\nsteam_app_id = 1\n";
        view.on_game(&read(DiskContent::Configured {
            config: DiskConfig::parse(text).unwrap(),
            hash: ConfigHash::of(text),
        }));
        assert_eq!(view.line(), "A: Doom");
        view.on_game(&GameEvent::Launched {
            drive: a(),
            name: "Doom".into(),
        });
        assert_eq!(view.line(), "A: Doom (launched)");
        view.on_floppy(&FloppyEvent::MediaRemoved {
            drive: a(),
            cause: RemovalCause::Ejected,
        });
        assert_eq!(view.line(), "A: no disk");
    }

    #[test]
    fn shows_problems_and_missing_drive() {
        let mut view = DriveView::new(a(), true);
        view.on_floppy(&FloppyEvent::DriveConnected { drive: a() });
        view.on_game(&read(DiskContent::NotConfigured));
        assert_eq!(view.line(), "A: disk not configured");
        view.on_game(&read(DiskContent::Unformatted));
        assert_eq!(view.line(), "A: disk can't be used");
        view.on_floppy(&FloppyEvent::DriveDisconnected { drive: a() });
        assert_eq!(view.line(), "Drive A: not found");
        view.set_detection(false);
        assert_eq!(view.line(), "Detection is off");
    }
}
