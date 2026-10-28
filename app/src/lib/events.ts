// What the backend sends to the UI (see app/src-tauri/src/events.rs).

/** Something that happened, as a code to translate plus its data. */
export interface AppEvent {
  /** Order of the events; also tells them apart. */
  seq: number;
  /** When it happened, in milliseconds since 1970. */
  at: number;
  code: string;
  params: Record<string, string>;
}

export interface AppInfo {
  drive: string;
  version: string;
}
