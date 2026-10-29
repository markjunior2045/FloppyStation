// What the backend sends and accepts (see app/src-tauri/src/view.rs and
// watcher.rs), and the calls to it.
import { invoke } from "@tauri-apps/api/core";

/** Something that happened, as a code to translate plus its data. */
export interface AppEvent {
  /** Order of the events; also tells them apart. */
  seq: number;
  /** When it happened, in milliseconds since 1970. */
  at: number;
  code: string;
  params: Record<string, string>;
}

export type DriveStatus = "off" | "starting" | "missing" | "empty" | "reading" | "present";

export interface ConfigView {
  name: string;
  kind: "steam" | "exe";
  appId: number | null;
  path: string | null;
  args: string[];
  process: string | null;
  hash: string;
  approved: boolean;
}

export interface DiskView {
  /** A `disk_*` code. */
  code: string;
  params: Record<string, string>;
  writeProtected: boolean | null;
  config: ConfigView | null;
}

export interface GameView {
  code: string;
  params: Record<string, string>;
}

export interface AppState {
  drive: string;
  detection: boolean;
  driveStatus: DriveStatus;
  line: string;
  disk: DiskView | null;
  game: GameView | null;
  launched: string | null;
  settingsError: string | null;
}

/** The settings file, as the backend reads and writes it. */
export interface Settings {
  version: number;
  drive: string;
  poll_interval_ms: number;
  empty_poll_interval_ms: number;
  on_removal: "close" | "force" | "none";
  close_timeout_s: number;
  on_drive_disconnect: "removal" | "ignore";
  detection_enabled: boolean;
  show_loading_box: boolean;
}

export const api = {
  state: () => invoke<AppState>("app_state"),
  recentEvents: () => invoke<AppEvent[]>("recent_events"),
  version: () => invoke<string>("app_version"),
  settings: () => invoke<Settings>("get_settings"),
  defaultSettings: () => invoke<Settings>("default_settings"),
  /** Rejects with a `settings_*` code. */
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  setDetection: (enabled: boolean) => invoke<void>("set_detection", { enabled }),
  removableDrives: () => invoke<string[]>("removable_drives"),
};
