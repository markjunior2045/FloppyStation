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
  /** `args` as one line, as typed in the form. */
  argsLine: string;
  process: string | null;
  hash: string;
  approved: boolean;
  /** Whether the program exists on this PC; always true for Steam games. */
  found: boolean;
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
  /** No settings saved yet: the Status tab shows the welcome. */
  firstRun: boolean;
}

/** Why a command failed, as a code to translate plus its data. */
export interface Problem {
  code: string;
  params: Record<string, string>;
}

/** An installed Steam game. */
export interface LibraryGame {
  appId: number;
  name: string;
}

/** An approved program. */
export interface TrustEntry {
  hash: string;
  name: string;
  path: string;
  /** When it was approved, in local time. */
  added: string;
}

/** The Disk tab's form, as the backend checks and writes it. */
export interface ConfigForm {
  kind: "steam" | "exe";
  name: string;
  appId: number | null;
  path: string;
  /** Arguments on one line, as typed. */
  args: string;
  process: string;
}

export type Tab = "status" | "disk" | "approvals" | "prefs";

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
  quiet_detection: boolean;
  notifications: boolean;
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
  finishWelcome: () => invoke<void>("finish_welcome"),
  /** The tab a newly opened window should show, if another place asked. */
  takeMainTab: () => invoke<Tab | null>("take_main_tab"),
  steamGames: () => invoke<LibraryGame[]>("steam_games"),
  openApplications: () => invoke<string[]>("open_applications"),
  /** A program chosen in a file dialog; null if cancelled. */
  pickProgram: () => invoke<string | null>("pick_program"),
  // The calls below reject with a Problem.
  configPreview: (form: ConfigForm) => invoke<string>("config_preview", { form }),
  writeConfig: (form: ConfigForm) => invoke<void>("write_config", { form }),
  removeConfig: () => invoke<void>("remove_config"),
  approvals: () => invoke<TrustEntry[]>("approvals"),
  approveDisk: () => invoke<void>("approve_disk"),
  revokeApproval: (hash: string) => invoke<void>("revoke_approval", { hash }),
};
