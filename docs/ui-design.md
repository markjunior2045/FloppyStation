# Floppy Station: UI design

Design of the Tauri app (stage B). It maps every state and code the library
returns to what the user sees, so the frontend (phases 5 to 7) has a single
reference. The UI can still change while it is built.

Status of this document: **step 1 of 3** (inventory, flows and states).
Wireframes (step 2) and the visual identity with a clickable mockup (step 3)
come next.

## Decisions

| Topic | Decision |
|---|---|
| Look | Windows XP theme with [XP.css](https://botoxparty.github.io/XP.css/) (MIT, CSS only). The window has no native frame: the XP title bar is drawn by the app and works as a drag region. Tahoma and Trebuchet MS, which ship with Windows 10 and 11. No dark theme. No Microsoft artwork (logo, wallpaper, sounds): the icon is our own floppy disk. |
| Framework | Svelte. |
| Languages | One JSON file per language, keyed by the codes in this document. English only at first. |
| Main window | One window, created on demand and destroyed when closed, around 760×520, with tabs: **Status**, **Disk**, **Approvals**, **Preferences**. |
| Loading box | A small XP-style box in the middle of the screen while a disk is read (see [Loading box](#loading-box)). On by default, can be turned off. |
| Notifications | Windows notifications only for what needs action: unapproved program, game not found, launch failed. Clicking one opens the right tab. |
| Focus | The app never opens or focuses a window by itself: games may be running full screen. The loading box shows on top without taking focus. |

## Surfaces

### Tray icon

Always present while the app runs: the floppy disk icon, which never changes.
Its tooltip and the first line of the menu show the current state in one line.

Menu:

1. State line (disabled), e.g. `A: Counter-Strike 2 (running)`.
2. **Open Floppy Station**: opens the main window on the Status tab.
3. **Detect disks** (check mark): turns detection on or off, saved in the
   preferences.
4. **Exit**.

Double-clicking the icon opens the main window.

Tray state line, by priority:

| Situation | Text |
|---|---|
| Detection off | `Detection is off` |
| Drive missing | `Drive A: not found` |
| Drive empty | `A: no disk` |
| Reading | `A: reading disk...` |
| Game launched from the disk | `A: <name> (launched)` |
| Disk configured, not launched (e.g. present at startup) | `A: <name>` |
| Disk not configured | `A: disk not configured` |
| Disk with a problem | `A: disk can't be used` |

### Main window

#### Status tab

What is going on now. Sections:

- **Drive**: letter and state (not found, empty, reading, disk in), with a
  hint when not found ("Plug the drive in, or pick another letter in
  Preferences").
- **Disk**: what the disk contains (see [Disk content](#disk-content)), with
  the action that fits: *Configure this disk*, *Review program*, *Edit*.
- **Game**: the game launched from the disk and its state (launched, closing,
  closed, left open) with the last relevant message (e.g. could not launch).
- **Detection** on/off switch, same as the tray.

The window title bar shows `Floppy Station`; the status bar at the bottom
shows the drive and detection state.

#### Disk tab

Configure the disk in the drive. Only usable with a disk in; otherwise it
shows "Insert a disk to configure it".

- **Current content** of the disk (same as Status), with *Remove
  configuration* when it has one.
- **Form**:
  1. **Type**: dropdown, `Steam game` or `Program`. Future kinds are added
     here.
  2. **Name**: filled in from the chosen game, editable, up to 100
     characters.
  3. Kind fields:
     - Steam game: pick from the installed library (searchable list read from
       the Steam manifests), or type an AppID.
     - Program: pick the `.exe` with a file dialog; optional arguments.
  4. **Process to close**: pick from the open applications (read only when
     this list is opened) or type a name. Empty with a Steam game shows a
     warning: "The game won't be closed when the disk is removed".
  5. **Preview** of the file that will be written (read only).
  6. **Write to disk**.
- Editing a configured disk fills the form with its values.
- Writing a program also approves it on this PC.

#### Approvals tab

Programs (`kind = "exe"`) approved on this PC.

- List: name, program path, date approved.
- **Revoke** per entry, with confirmation.
- Empty state: "No approved programs. Programs written to a disk with
  Floppy Station are approved automatically."
- When the disk in the drive holds an unapproved program, a box at the top
  shows what it would run (program, arguments, process to close) with
  **Approve** and **Not now**. This is where the notification and the
  loading box lead.

#### Preferences tab

| Group | Setting | Control |
|---|---|---|
| Drive | Drive letter | Dropdown of removable drives (drive type only) |
| Detection | Detect disks | Checkbox |
| Detection | Check every (disk in) | Slider or number, 250 to 10000 ms |
| Detection | Check every (empty drive) | Slider or number, same range, with "an empty drive clicks on every check" |
| When the disk is removed | Ask the game to close, force it after N s / Force it to close / Leave it open | Option buttons, plus N (1 to 60 s) |
| When the drive is unplugged | Same as removing the disk / Leave the game open | Option buttons |
| Display | Show the loading box | Checkbox |
| Startup | Start with Windows | Checkbox (implemented in phase 8) |

Changes are saved right away; detection settings apply without restarting.

#### First run

When there is no settings file, the Status tab starts with a welcome box:

1. "Pick your floppy drive" with the drive dropdown (pre-selects `A:` if it
   is removable).
2. "Insert a disk": waits for a disk and shows what it found, with
   *Configure this disk* if it is empty.
3. Short note on how it works: insert to play, remove to close.

### Loading box

A small XP window (about 320×110) in the middle of the screen, on top of
other windows, without taking focus or showing in the taskbar.

- Text: **Reading floppy disk...** with an indeterminate XP progress bar.
  Generic on purpose: the game's name is not shown.
- Opens when a disk is detected (`media_inserted`, also when the drive is
  plugged back in with a disk). Not shown for a disk present at startup.
- Closes:
  - about 3 s after the game is launched (`game_launched`), since the app
    does not know when the game finishes opening;
  - right away for a disk without a configuration, or when the game is
    taken back after a drive unplug (`game_reattached`);
  - after showing a message, for the cases below.
- Messages instead of the bar (box stays until closed, or 10 s):

| Case | Message | Button |
|---|---|---|
| Disk not configured | This disk has no game yet. | Configure |
| Program not approved | This disk wants to run a program you haven't approved. | Review |
| Game not found | The program on this disk isn't installed on this PC. | - |
| Launch failed | The game could not be started. | Details |
| Disk can't be used | This disk can't be read. | Details |

Buttons open the main window on the right tab. The box is created when needed
and destroyed when it closes.

### Notifications

Sent only for what needs the user, and only when the loading box is off or
already closed:

| Code | Notification | Click opens |
|---|---|---|
| `launch_untrusted` | Unapproved program on the disk in A: | Approvals |
| `launch_not_found` | The program on the disk isn't installed on this PC | Disk |
| `launch_failed`, `launch_steam_missing`, `launch_elevation_required` | Couldn't start the game | Status |
| `close_failed` | A game could not be closed | Status |

## States and codes

Every code the library returns today, where it shows up and its English text.
The text column is the base for the language file.

### Drive

From `floppy::FloppyEvent` and the monitor state.

| Code / state | Meaning | Status tab | Tray |
|---|---|---|---|
| `drive_connected` | Drive is there | Drive: connected | - |
| `drive_disconnected` | Drive letter not found | Drive not found, with the hint | `Drive A: not found` |
| `media_inserted` | A disk was detected; reading starts | Reading... | `A: reading disk...` |
| `media_removed` | Disk taken out (or drive unplugged with it) | Drive: no disk | `A: no disk` |
| *(new)* detection off | Detection turned off | Detection is off | `Detection is off` |

### Disk content

From `diskconfig::DiskContent` (`disk_read` carries it).

| Code | Text | Action |
|---|---|---|
| `disk_configured` | `<name>`: Steam game `<AppID>` / Program `<path>` | Edit |
| `disk_not_configured` | This disk has no game yet. | Configure this disk |
| `disk_config_invalid` | The game file on this disk is damaged: `<detail>` | Configure this disk (overwrites it) |
| `disk_unsupported_version` | This disk was set up by a newer version of Floppy Station. | - |
| `disk_unsupported_kind` | This disk uses a game type this version doesn't know (`<kind>`). | - |
| `disk_unformatted` | This disk isn't formatted. Format it in File Explorer first. | - |
| `disk_unreadable` | This disk can't be read. It may be damaged. | - |
| `disk_none` | No disk in the drive. | - |

Write protection (`write_protected`) is shown as a note: "This disk is write
protected: slide the tab to change it."

Details for `disk_config_invalid` (`diskconfig::ConfigError`), shown in a
smaller line:

| Code | Text |
|---|---|
| `config_too_large` | The file is too large. |
| `config_not_utf8` | The file isn't text. |
| `config_syntax` | The file has a syntax error: `<message>` |
| `config_missing_field` | `<field>` is missing. |
| `config_wrong_type` | `<field>` must be `<type>`. |
| `config_invalid_value` | `<field>` is not valid: `<reason>` |

Reasons: `empty` (is empty), `too_long` (is too long), `not_positive` (must be
a positive number), `out_of_range` (is out of range), `not_absolute_path`
(must be a full path), `not_exe` (must be an .exe file), `has_directory` (must
be a file name, without folders).

### Writing and removing a configuration

From `diskconfig::WriteError` (Disk tab).

| Code | Text |
|---|---|
| *(success)* | Written to the disk. |
| `disk_write_protected` | The disk is write protected. Slide the tab and try again. |
| `disk_in_use` | Another program is using the disk. Close File Explorer windows on it and try again. |
| `disk_full` | The disk is full. |
| `disk_removed` | The disk was removed while writing. Insert it and try again. |
| `disk_write_failed` | The file could not be verified after writing. Try again or use another disk. |
| `disk_io` | The disk could not be written: `<detail>` |

While writing (about 3 to 7 s) the form is disabled and an XP progress bar
shows "Writing to the disk...".

### Games

From `controller::GameEvent` and `launcher`.

| Code | Text (Status tab) | Loading box |
|---|---|---|
| `disk_read` | *(updates the Disk section)* | - |
| `game_ignored_at_startup` | The disk was already in the drive when Floppy Station started. Remove and insert it to play. | not shown |
| `launch_untrusted` | This disk wants to run a program you haven't approved. | message + Review |
| `game_launched` | `<name>` launched. | closes after 3 s |
| `game_reattached` | `<name>` is still open from before the drive was unplugged. | closes |
| `launch_failed` + reason | Couldn't start `<name>`: `<reason>` | message |
| `game_left_open` + reason | `<name>` was left open: `<reason>` | - |
| `game_closing` | Closing `<name>`... | - |
| `game_closed` + outcome | `<outcome>` | - |

Launch errors (`launcher::LaunchError`):

| Code | Text |
|---|---|
| `launch_not_found` | The program isn't installed on this PC (the disk may come from another PC). |
| `launch_elevation_required` | The program needs administrator rights. Floppy Station doesn't start those, because it couldn't close them. |
| `launch_steam_missing` | Steam isn't installed. |
| `launch_failed` | Windows couldn't start it (error `<code>`). |

Left-open reasons (`controller::LeftOpenReason`):

| Code | Text |
|---|---|
| `left_open_setting` | your preferences say to leave games open. |
| `left_open_drive_disconnected` | the drive was unplugged, and your preferences say to leave the game open. |
| `left_open_no_process` | no process to close is set for this game. |

Close outcomes (`launcher::CloseOutcome`):

| Code | Text |
|---|---|
| `close_done` | `<name>` closed. |
| `close_forced` | `<name>` was forced to close. |
| `close_already_closed` | `<name>` was already closed. |
| `close_failed` | `<name>` could not be closed (it may be running as administrator). |

### Local files

From `settings::SettingsError` and `trust::TrustError`. Shown as a banner on
the Status tab; the app keeps working with defaults.

| Code | Text |
|---|---|
| `settings_appdata_unavailable`, `trust_appdata_unavailable` | Floppy Station can't find its settings folder. |
| `settings_invalid` | The settings file is damaged; default settings are in use. |
| `trust_invalid` | The approved programs file is damaged; no program is approved until it is fixed. |
| `settings_io`, `trust_io` | Floppy Station can't read or save its files: `<detail>` |
| `settings_serialize`, `trust_serialize` | Floppy Station couldn't save its settings. |

## Flows

### Configure a disk from scratch

1. Insert a blank (formatted) disk. The loading box shows *Reading floppy
   disk...*, then *This disk has no game yet* with **Configure**.
2. **Configure** opens the Disk tab with the form.
3. Pick the type (*Steam game*), the game from the library, and the process
   from the open applications (or leave it empty and see the warning).
4. Check the preview and click **Write to disk**. The progress bar shows
   while writing, then "Written to the disk".
5. Remove the disk and insert it again: the game launches.

### Change the game on a disk

Insert the disk, open Disk, change the form, **Write to disk**. A program
written this way is approved again with its new contents.

### Approve a program from another PC

1. Insert the disk. The box shows *This disk wants to run a program you
   haven't approved* with **Review** (or a notification, if the box is off).
2. **Review** opens Approvals with the program, its arguments and the process
   to close, and a warning if the program isn't on this PC.
3. **Approve**, then remove and insert the disk to launch it.

### Drive not found

The tray says `Drive A: not found` and the Status tab explains it. The user
plugs the drive in (detected again within about 20 s) or picks another letter
in Preferences.

### Disk can't be read

The box (or Status) says the disk can't be read, with a hint to format it in
File Explorer. Floppy Station never formats disks.

## Backend additions needed

Found while mapping the screens. To be added in phases 5 to 7.

| Need | Used by |
|---|---|
| Detection on/off, saved in the preferences, applied without restarting | Tray, Status, Preferences |
| Apply interval and drive changes without restarting | Preferences |
| Current state per drive (drive, disk content, game), not only events | Status, tray line, opening the window mid-session |
| Steam library: installed games (AppID, name) from the Steam manifests | Disk tab |
| Open applications (process names), read only when the list opens | Disk tab |
| Removable drives (letter, drive type only) | Preferences, first run |
| Approve and revoke from the UI; details of the unapproved disk | Approvals |
| Setting `show_loading_box` | Preferences, loading box |
| A "reading" moment between `media_inserted` and `disk_read` | Loading box, tray |
