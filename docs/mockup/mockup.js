// Floppy Station clickable mockup. Simulates the app's states with fake
// disks and timings; nothing here talks to a real drive.
"use strict";

// Shorter than the real ~6 s, so the mockup is quick to click through.
const READ_MS = 2500;
const WRITE_MS = 2500;
const LAUNCH_CLOSE_MS = 3000;
const MESSAGE_CLOSE_MS = 10000;
const CLOSING_MS = 1500;

const LIBRARY = [
  { name: "Counter-Strike 2", appId: 730, process: "cs2.exe" },
  { name: "Half-Life 2", appId: 220, process: "hl2.exe" },
  { name: "Portal 2", appId: 620, process: "portal2.exe" },
  { name: "Stardew Valley", appId: 413150, process: "Stardew Valley.exe" },
  { name: "Tabletop Simulator", appId: 286160, process: "Tabletop Simulator.exe" },
];

const OPEN_APPS = [
  "cs2.exe",
  "Discord.exe",
  "explorer.exe",
  "steam.exe",
  "steamwebhelper.exe",
  "Tabletop Simulator.exe",
];

// The fake disks that can be inserted.
const DISKS = {
  cs2: {
    label: "Counter-Strike 2 (Steam)",
    content: "configured",
    config: { name: "Counter-Strike 2", kind: "steam", appId: 730, process: "cs2.exe" },
  },
  tts: {
    label: "Tabletop Simulator, no process",
    content: "configured",
    config: { name: "Tabletop Simulator", kind: "steam", appId: 286160, process: "" },
  },
  charmap: {
    label: "Program not approved",
    content: "configured",
    config: {
      name: "Character Map",
      kind: "exe",
      path: "C:\\Windows\\System32\\charmap.exe",
      args: [],
      process: "",
    },
  },
  doom: {
    label: "Program from another PC",
    content: "configured",
    missing: true,
    config: {
      name: "Doom",
      kind: "exe",
      path: "C:\\Games\\Doom\\doom.exe",
      args: ["-fullscreen"],
      process: "",
    },
  },
  blank: { label: "Blank disk", content: "not_configured" },
  locked: { label: "Blank disk, write protected", content: "not_configured", writeProtected: true },
  newer: { label: "Disk from a newer version", content: "unsupported_version" },
  damaged: { label: "Damaged disk", content: "unreadable" },
};

const state = {
  tab: "status",
  windowOpen: true,
  running: true,
  firstRun: false,
  banner: false,
  driveConnected: true,
  detection: true,
  diskId: null,
  unpluggedWith: null,
  reading: false,
  game: null, // { name, diskId, status, note, closable }
  approvals: [
    { key: approvalKey({ name: "Notepad", path: "C:\\Windows\\System32\\notepad.exe", args: [] }), name: "Notepad", path: "C:\\Windows\\System32\\notepad.exe", date: "2026-10-27" },
    { key: approvalKey(DISKS.doom.config), name: "Doom", path: "C:\\Games\\Doom\\doom.exe", date: "2026-10-28" },
  ],
  selectedApproval: null,
  prefs: {
    drive: "A:",
    pollMs: 1000,
    emptyMs: 1000,
    onRemoval: "close",
    timeout: 10,
    onUnplug: "removal",
    loadingBox: true,
    startup: false,
  },
  form: emptyForm(),
  writing: false,
  writeResult: null, // { ok, text }
  trayOpen: false,
};

const timers = { read: null, box: null, closing: null };

function emptyForm() {
  return { kind: "steam", search: "", appId: null, name: "", process: "", path: "", args: "" };
}

function approvalKey(config) {
  return [config.name, config.path, (config.args || []).join(" "), config.process || ""].join("|");
}

function isApproved(config) {
  return state.approvals.some((entry) => entry.key === approvalKey(config));
}

function disk() {
  return state.diskId ? DISKS[state.diskId] : null;
}

function escape(text) {
  return String(text).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

function describeTarget(config) {
  if (config.kind === "steam") {
    return `Steam game ${config.appId}`;
  }
  const args = config.args && config.args.length ? ` ${config.args.join(" ")}` : "";
  return `Program ${config.path}${args}`;
}

function describeClose(config) {
  if (config.process) {
    return `closes ${config.process}`;
  }
  return config.kind === "steam" ? "not closed on removal" : "closes the program itself";
}

// ---------------------------------------------------------------------
// Simulated events

function insertDisk(id, reconnected = false) {
  if (!state.running || !state.driveConnected) {
    return;
  }
  if (state.diskId) {
    removeDisk("ejected");
  }
  state.diskId = id;
  state.writeResult = null;
  if (!state.detection) {
    render();
    return;
  }
  state.reading = true;
  showBox("reading");
  render();
  clearTimeout(timers.read);
  timers.read = setTimeout(() => diskRead(reconnected), READ_MS);
}

function diskRead(reconnected) {
  state.reading = false;
  const d = disk();
  if (!d) {
    return;
  }
  if (d.content === "not_configured") {
    showBox("blank");
  } else if (d.content === "unreadable") {
    showBox("damaged");
  } else if (d.content === "unsupported_version") {
    showBox("newer");
  } else if (d.config.kind === "exe" && !isApproved(d.config)) {
    showBox("untrusted");
  } else if (reconnected && state.game && state.game.diskId === state.diskId && state.game.status === "left_open") {
    state.game.status = "launched";
    state.game.note = "still open from before the drive was unplugged";
    hideBox();
  } else if (d.missing) {
    state.game = { name: d.config.name, diskId: state.diskId, status: "failed", note: "The program isn't installed on this PC (the disk may come from another PC)." };
    showBox("missing");
  } else {
    state.game = {
      name: d.config.name,
      diskId: state.diskId,
      status: "launched",
      note: "",
      closable: d.config.kind === "exe" || Boolean(d.config.process),
    };
    clearTimeout(timers.box);
    timers.box = setTimeout(hideBox, LAUNCH_CLOSE_MS);
  }
  render();
}

function removeDisk(cause) {
  clearTimeout(timers.read);
  if (state.reading) {
    state.reading = false;
    hideBox();
  }
  const game = state.game;
  if (game && game.status === "launched" && game.diskId === state.diskId) {
    const p = state.prefs;
    if (cause === "unplugged" && p.onUnplug === "ignore") {
      game.status = "left_open";
      game.note = "the drive was unplugged, and your preferences say to leave the game open.";
    } else if (p.onRemoval === "none") {
      game.status = "left_open";
      game.note = "your preferences say to leave games open.";
    } else if (!game.closable) {
      game.status = "left_open";
      game.note = "no process to close is set for this game.";
    } else {
      game.status = "closing";
      game.note = p.onRemoval === "force" ? "Forcing it to close..." : `Asking it to close, forcing it after ${p.timeout} s...`;
      clearTimeout(timers.closing);
      timers.closing = setTimeout(() => {
        game.status = "closed";
        game.note = p.onRemoval === "force" ? "was forced to close." : "closed.";
        render();
      }, CLOSING_MS);
    }
  }
  state.diskId = null;
  state.writing = false;
  render();
}

function unplugDrive() {
  if (!state.driveConnected) {
    return;
  }
  state.unpluggedWith = state.diskId;
  if (state.diskId) {
    removeDisk("unplugged");
  }
  state.driveConnected = false;
  render();
}

function plugDrive() {
  if (state.driveConnected) {
    return;
  }
  state.driveConnected = true;
  const id = state.unpluggedWith;
  state.unpluggedWith = null;
  render();
  if (id) {
    insertDisk(id, true);
  }
}

// ---------------------------------------------------------------------
// Loading box

const BOX = {
  reading: null,
  blank: { icon: "info", text: "This disk has no game yet.", button: "Configure", tab: "disk" },
  untrusted: { icon: "warn", text: "This disk wants to run a program you haven't approved.", button: "Review", tab: "approvals" },
  missing: { icon: "warn", text: "The program on this disk isn't installed on this PC.", button: null },
  damaged: { icon: "warn", text: "This disk can't be read. It may be damaged.", button: "Details", tab: "status" },
  newer: { icon: "info", text: "This disk was set up by a newer version of Floppy Station.", button: "Details", tab: "status" },
};

function showBox(kind) {
  if (!state.prefs.loadingBox) {
    return;
  }
  const box = document.getElementById("loading-box");
  const body = document.getElementById("loading-body");
  const controls = document.getElementById("loading-controls");
  clearTimeout(timers.box);
  if (kind === "reading") {
    controls.innerHTML = "";
    body.innerHTML = `<p style="margin:0 0 8px">Reading floppy disk...</p>${marquee()}`;
  } else {
    const m = BOX[kind];
    controls.innerHTML = `<button aria-label="Close" data-action="hide-box"></button>`;
    const icon = m.icon === "warn" ? warnIcon() : infoIcon();
    // A write protected disk can't be configured.
    const locked = kind === "blank" && disk() && disk().writeProtected;
    const note = locked ? `<br><span class="muted">It is write protected: slide the tab to configure it.</span>` : "";
    const button = m.button
      ? `<div class="actions"><button data-action="box-open" data-tab="${m.tab}" ${locked ? "disabled" : ""}>${m.button}</button></div>`
      : "";
    body.innerHTML = `<div class="message">${icon}<span>${escape(m.text)}${note}</span></div>${button}`;
    timers.box = setTimeout(hideBox, MESSAGE_CLOSE_MS);
  }
  box.hidden = false;
}

function hideBox() {
  clearTimeout(timers.box);
  document.getElementById("loading-box").hidden = true;
}

function warnIcon() {
  return `<svg width="24" height="24" viewBox="0 0 24 24"><path d="M12 2 23 21H1z" fill="#f5c400" stroke="#8a6d00"/><rect x="11" y="8" width="2" height="7" fill="#222"/><rect x="11" y="17" width="2" height="2" fill="#222"/></svg>`;
}

function infoIcon() {
  return `<svg width="24" height="24" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10.5" fill="#2f6fde" stroke="#173f9a"/><rect x="11" y="10" width="2" height="8" fill="#fff"/><rect x="11" y="6" width="2" height="2" fill="#fff"/></svg>`;
}

// ---------------------------------------------------------------------
// Rendering

function trayLine() {
  const d = disk();
  const drive = state.prefs.drive;
  if (!state.detection) return "Detection is off";
  if (!state.driveConnected) return `Drive ${drive} not found`;
  if (!d) return `${drive} no disk`;
  if (state.reading) return `${drive} reading disk...`;
  if (state.game && state.game.diskId === state.diskId && state.game.status === "launched") {
    return `${drive} ${state.game.name} (launched)`;
  }
  if (d.content === "configured") return `${drive} ${d.config.name}`;
  if (d.content === "not_configured") return `${drive} disk not configured`;
  return `${drive} disk can't be used`;
}

function render() {
  renderControls();
  const main = document.getElementById("main-window");
  main.hidden = !state.windowOpen || !state.running;
  for (const tab of document.querySelectorAll("#tabs [role=tab]")) {
    tab.setAttribute("aria-selected", String(tab.dataset.tab === state.tab));
  }
  const panel = document.getElementById("panel");
  panel.innerHTML = { status: renderStatus, disk: renderDisk, approvals: renderApprovals, prefs: renderPrefs }[state.tab]();
  document.getElementById("status-bar").innerHTML =
    `<p class="status-bar-field">${escape(driveSummary())}</p>` +
    `<p class="status-bar-field">Detection ${state.detection ? "on" : "off"}</p>` +
    `<p class="status-bar-field">Version 0.1.0</p>`;
  document.getElementById("tray-icon").title = `Floppy Station - ${trayLine()}`;
  renderTray();
}

function driveSummary() {
  const drive = state.prefs.drive;
  if (!state.driveConnected) return `${drive} not found`;
  if (!state.diskId) return `${drive} no disk`;
  return state.reading ? `${drive} reading` : `${drive} disk in`;
}

function renderStatus() {
  if (state.firstRun) {
    return renderFirstRun();
  }
  const drive = state.prefs.drive;
  const banner = state.banner
    ? `<div class="banner"><span>${warnIconSmall()} The settings file is damaged; default settings are in use.</span><button data-action="dismiss-banner">Close</button></div>`
    : "";

  let driveBox;
  if (!state.driveConnected) {
    driveBox = `<div class="row"><span class="drive-icon missing"></span><div>
      <div class="title warn">Drive ${drive} not found</div>
      <div>Plug the drive in, or pick another letter in Preferences.</div></div>
      <span style="flex:1"></span><button data-action="tab" data-tab="prefs">Preferences</button></div>`;
  } else {
    driveBox = `<div class="row"><span class="drive-icon"></span><div>
      <div class="title">${drive}</div><div>${state.diskId ? "Disk in the drive" : "No disk in the drive"}</div></div></div>`;
  }

  return `${banner}
    <fieldset><legend>Drive</legend>${driveBox}</fieldset>
    <fieldset><legend>Disk</legend>${renderDiskSummary(true)}</fieldset>
    <fieldset><legend>Game</legend>${renderGame()}</fieldset>
    <input type="checkbox" id="detect" data-action="toggle-detection" ${state.detection ? "checked" : ""}>
    <label for="detect">Detect disks</label>`;
}

function renderDiskSummary(withActions) {
  const d = disk();
  if (!state.detection && d) {
    return `<div class="muted">Detection is off: the disk isn't read.</div>`;
  }
  if (!d) {
    return `<div class="muted">No disk.</div>`;
  }
  if (state.reading) {
    return `<div style="margin-bottom:6px">Reading floppy disk...</div>${marquee()}`;
  }
  // Configuring or editing writes to the disk: not possible while it is
  // write protected.
  const action = (label, tab, writes = false) =>
    withActions
      ? `<span style="flex:1"></span><button data-action="tab" data-tab="${tab}" ${writes && d.writeProtected ? "disabled" : ""}>${label}</button>`
      : "";
  const protectedNote = d.writeProtected
    ? `<div class="muted" style="margin-top:4px">This disk is write protected: slide the tab to change it.</div>`
    : "";
  switch (d.content) {
    case "configured": {
      const c = d.config;
      if (c.kind === "exe" && !isApproved(c)) {
        return `<div class="row"><img class="big-icon" src="icon.svg" alt=""><div>
          <div class="title">${warnIconSmall()} ${escape(c.name)}</div>
          <div>This disk wants to run a program you haven't approved.</div></div>${action("Review...", "approvals")}</div>`;
      }
      return `<div class="row"><img class="big-icon" src="icon.svg" alt=""><div>
        <div class="title">${escape(c.name)}</div>
        <div>${escape(describeTarget(c))} - ${escape(describeClose(c))}</div></div>${action("Edit...", "disk", true)}</div>${protectedNote}`;
    }
    case "not_configured":
      return `<div class="row"><img class="big-icon" src="icon.svg" alt=""><div>This disk has no game yet.</div>${action("Configure this disk", "disk", true)}</div>${protectedNote}`;
    case "unreadable":
      return `<div class="row"><img class="big-icon" src="icon.svg" alt="" style="opacity:.5"><div>
        <div class="title warn">This disk can't be read. It may be damaged.</div>
        <div>Try formatting it in File Explorer. Floppy Station never formats disks.</div></div></div>`;
    case "unsupported_version":
      return `<div class="row"><img class="big-icon" src="icon.svg" alt=""><div>This disk was set up by a newer version of Floppy Station.</div></div>`;
    default:
      return "";
  }
}

function renderGame() {
  const g = state.game;
  if (!g) {
    return `<div class="muted">No game launched yet. Insert a configured disk to play.</div>`;
  }
  const p = state.prefs;
  switch (g.status) {
    case "launched": {
      const removal = {
        close: `Removing the disk asks it to close, and forces it after ${p.timeout} s.`,
        force: "Removing the disk forces it to close.",
        none: "Removing the disk leaves it open.",
      }[p.onRemoval];
      const note = g.note ? `${escape(g.name)} is ${escape(g.note)}.` : `${escape(g.name)} launched.`;
      return `<div class="ok title">${note}</div><div>${g.closable ? removal : "It won't be closed when the disk is removed (no process to close is set)."}</div>`;
    }
    case "closing":
      return `<div class="title">Closing ${escape(g.name)}...</div><div>${escape(g.note)}</div>`;
    case "closed":
      return `<div>${escape(g.name)} ${escape(g.note)}</div>`;
    case "left_open":
      return `<div>${escape(g.name)} was left open: ${escape(g.note)}</div>`;
    case "failed":
      return `<div class="warn title">Couldn't start ${escape(g.name)}</div><div>${escape(g.note)}</div>`;
    default:
      return "";
  }
}

function renderFirstRun() {
  const d = disk();
  let step2 = `<span class="muted">Waiting for a disk...</span>`;
  if (d && state.reading) {
    step2 = `Reading floppy disk...`;
  } else if (d && d.content === "not_configured") {
    step2 = `This disk has no game yet. <button data-action="tab" data-tab="disk">Configure this disk</button>`;
  } else if (d && d.content === "configured") {
    step2 = `<span class="ok">Found ${escape(d.config.name)}.</span>`;
  } else if (d) {
    step2 = `<span class="warn">This disk can't be used. Try another one.</span>`;
  }
  return `<fieldset><legend>Welcome to Floppy Station</legend>
      <p style="margin-top:0">Insert a disk to play, remove it to close the game.</p>
      <div class="form-grid" style="grid-template-columns:170px 1fr">
        <span>1. Pick your floppy drive:</span>
        <select data-action="pref" data-pref="drive">${driveOptions()}</select>
        <span>2. Insert a disk.</span>
        <span>${step2}</span>
      </div>
      <div class="actions"><button data-action="finish-first-run">Done</button></div>
    </fieldset>`;
}

function driveOptions() {
  // Only removable drives are listed.
  return ["A:", "B:", "E:"]
    .map((letter) => `<option value="${letter}" ${letter === state.prefs.drive ? "selected" : ""}>${letter} (removable)</option>`)
    .join("");
}

function renderDisk() {
  const d = disk();
  if (!state.driveConnected || !d) {
    return `<div class="row" style="justify-content:center;height:100%">
      <img class="big-icon" src="icon.svg" alt=""><span>Insert a disk to configure it.</span></div>`;
  }
  if (state.reading) {
    return `<fieldset><legend>Disk in ${state.prefs.drive}</legend>${renderDiskSummary(false)}</fieldset>`;
  }
  const f = state.form;
  const disabled = state.writing || d.writeProtected ? "disabled" : "";
  let fields;
  if (f.kind === "steam") {
    const items = LIBRARY.filter((g) => g.name.toLowerCase().includes(f.search.toLowerCase()))
      .map((g) => `<div data-action="pick-game" data-app="${g.appId}" class="${f.appId === g.appId ? "selected" : ""}"><span>${escape(g.name)}</span><span>${g.appId}</span></div>`)
      .join("");
    fields = `
      <label>Game:</label><input type="text" placeholder="Search your Steam library" value="${escape(f.search)}" data-action="form" data-field="search" ${disabled}>
      <span></span><div class="list">${items || '<div class="muted">No game matches.</div>'}</div>`;
  } else {
    fields = `
      <label>Program:</label><div class="with-button"><input type="text" value="${escape(f.path)}" data-action="form" data-field="path" ${disabled}><button data-action="browse" ${disabled}>Browse...</button></div>
      <label>Arguments:</label><input type="text" value="${escape(f.args)}" data-action="form" data-field="args" ${disabled}>`;
  }
  const processHint = `<span></span><span id="process-hint">${processHintHtml()}</span>`;

  const current = `<fieldset><legend>Disk in ${state.prefs.drive}</legend>
      <div class="row"><div>${renderDiskSummary(false)}</div><span style="flex:1"></span>
      ${d.content === "configured" ? `<button data-action="remove-config" ${disabled}>Remove configuration</button>` : ""}</div></fieldset>`;

  const writing = state.writing
    ? `<div style="margin:8px 0 4px">Writing to the disk...</div>${marquee()}`
    : "";
  const result = state.writeResult
    ? `<div class="result ${state.writeResult.ok ? "ok" : "warn"}">${state.writeResult.ok ? "" : warnIconSmall() + " "}${escape(state.writeResult.text)}</div>`
    : "";

  return `${current}
    <fieldset><legend>Game</legend>
      <div class="form-grid">
        <label>Type:</label>
        <select data-action="form" data-field="kind" ${disabled}>
          <option value="steam" ${f.kind === "steam" ? "selected" : ""}>Steam game</option>
          <option value="exe" ${f.kind === "exe" ? "selected" : ""}>Program</option>
        </select>
        ${fields}
        <label>Name:</label><input type="text" maxlength="100" value="${escape(f.name)}" data-action="form" data-field="name" ${disabled}>
        <label>Close:</label><div class="with-button"><input type="text" value="${escape(f.process)}" data-action="form" data-field="process" placeholder="e.g. cs2.exe" ${disabled}><button data-action="pick-process" ${disabled}>Pick...</button></div>
        ${processHint}
      </div>
    </fieldset>
    <div class="row spread">
      <button data-action="preview" ${disabled || (formConfig() ? "" : "disabled")}>Preview...</button>
      <button data-action="write" ${disabled || (formConfig() ? "" : "disabled")}>Write to disk</button>
    </div>
    ${writing}${result}`;
}

function processHintHtml() {
  const f = state.form;
  if (f.process.trim()) return "";
  return f.kind === "steam"
    ? `<span class="warn hint">${warnIconSmall()} The game won't be closed when the disk is removed.</span>`
    : `<span class="muted hint">Empty: closes the program itself.</span>`;
}

function formConfig() {
  const f = state.form;
  const name = f.name.trim();
  if (!name) return null;
  if (f.kind === "steam") {
    if (!f.appId) return null;
    return { name, kind: "steam", appId: f.appId, process: f.process.trim() };
  }
  if (!/^[a-z]:\\.+\.exe$/i.test(f.path.trim())) return null;
  const args = f.args.trim() ? f.args.trim().split(/\s+/) : [];
  return { name, kind: "exe", path: f.path.trim(), args, process: f.process.trim() };
}

function toToml(c) {
  const quote = (s) => JSON.stringify(s);
  const lines = ["# Floppy Station disk configuration.", "version = 1", "", "[game]", `name = ${quote(c.name)}`, `kind = ${quote(c.kind)}`];
  if (c.kind === "steam") {
    lines.push(`steam_app_id = ${c.appId}`);
  } else {
    lines.push(`path = '${c.path}'`);
    if (c.args.length) lines.push(`args = [${c.args.map(quote).join(", ")}]`);
  }
  if (c.process) lines.push(`process = ${quote(c.process)}`);
  return lines.join("\n");
}

function renderApprovals() {
  const d = disk();
  let pending = "";
  if (d && !state.reading && d.content === "configured" && d.config.kind === "exe" && !isApproved(d.config)) {
    const c = d.config;
    pending = `<fieldset class="pending"><legend>Disk in ${state.prefs.drive} wants to run</legend>
      <div class="title">${warnIconSmall()} ${escape(c.name)}</div>
      <dl>
        <dt>Program:</dt><dd>${escape(c.path)}</dd>
        <dt>Arguments:</dt><dd>${c.args.length ? escape(c.args.join(" ")) : "(none)"}</dd>
        <dt>Closes:</dt><dd>${c.process ? escape(c.process) : "the program itself"}</dd>
      </dl>
      <div class="row"><span class="muted">Only approve programs you trust.</span><span style="flex:1"></span>
        <button data-action="approve">Approve</button><button data-action="not-now">Not now</button></div>
    </fieldset>`;
  }
  const rows = state.approvals
    .map((e, i) => `<tr data-action="select-approval" data-index="${i}" class="${state.selectedApproval === i ? "selected" : ""}">
        <td>${escape(e.name)}</td><td title="${escape(e.path)}">${escape(e.path)}</td><td>${e.date}</td></tr>`)
    .join("");
  const table = state.approvals.length
    ? `<div class="table-wrap"><table class="approvals"><thead><tr><th>Name</th><th>Program</th><th>Approved</th></tr></thead><tbody>${rows}</tbody></table></div>`
    : `<div class="table-wrap muted" style="display:flex;align-items:center;justify-content:center;padding:0 20px;text-align:center">No approved programs. Programs written to a disk with Floppy Station are approved automatically.</div>`;
  return `${pending}
    <p style="margin:0 0 4px">Approved programs:</p>
    ${table}
    <div class="actions"><button data-action="revoke" ${state.selectedApproval === null ? "disabled" : ""}>Revoke</button></div>`;
}

function renderPrefs() {
  const p = state.prefs;
  const radio = (name, value, label, extra = "") =>
    `<div class="field-row"><input type="radio" id="${name}-${value}" name="${name}" data-action="pref" data-pref="${name}" value="${value}" ${p[name] === value ? "checked" : ""}><label for="${name}-${value}">${label}</label>${extra}</div>`;
  return `<div class="prefs">
    <div>
      <fieldset><legend>Drive</legend>
        <div class="field-row"><label for="drive">Drive:</label><select id="drive" data-action="pref" data-pref="drive">${driveOptions()}</select></div>
      </fieldset>
      <fieldset><legend>Detection</legend>
        <div class="field-row"><input type="checkbox" id="pref-detect" data-action="toggle-detection" ${state.detection ? "checked" : ""}><label for="pref-detect">Detect disks</label></div>
        <p style="margin:8px 0 4px">Check the drive every:</p>
        <div class="field-row indent"><input type="number" min="250" max="10000" step="250" value="${p.pollMs}" data-action="pref" data-pref="pollMs"><label>ms with a disk in</label></div>
        <div class="field-row indent"><input type="number" min="250" max="10000" step="250" value="${p.emptyMs}" data-action="pref" data-pref="emptyMs"><label>ms when empty</label></div>
        <p class="muted indent" style="margin:4px 0 0 20px">An empty drive clicks on every check. Longer intervals click less but notice disks later.</p>
      </fieldset>
    </div>
    <div>
      <fieldset><legend>When the disk is removed</legend>
        ${radio("onRemoval", "close", "Ask the game to close, force it after", `<input type="number" min="1" max="60" value="${p.timeout}" data-action="pref" data-pref="timeout" style="width:40px"><label>s</label>`)}
        ${radio("onRemoval", "force", "Force it to close")}
        ${radio("onRemoval", "none", "Leave it open")}
      </fieldset>
      <fieldset><legend>When the drive is unplugged</legend>
        ${radio("onUnplug", "removal", "Same as removing the disk")}
        ${radio("onUnplug", "ignore", "Leave the game open")}
      </fieldset>
      <fieldset><legend>Display and startup</legend>
        <div class="field-row"><input type="checkbox" id="pref-box" data-action="pref" data-pref="loadingBox" ${p.loadingBox ? "checked" : ""}><label for="pref-box">Show the loading box</label></div>
        <div class="field-row"><input type="checkbox" id="pref-startup" data-action="pref" data-pref="startup" ${p.startup ? "checked" : ""}><label for="pref-startup">Start with Windows</label></div>
      </fieldset>
    </div>
  </div>`;
}

// XP's indeterminate progress bar. XP.css animates it with pseudo-elements
// that Chromium (and so WebView2) doesn't animate on <progress>.
function marquee() {
  return `<div class="marquee" role="progressbar" aria-label="Working"><span></span></div>`;
}

function warnIconSmall() {
  return `<svg width="12" height="12" viewBox="0 0 24 24" style="vertical-align:-2px"><path d="M12 2 23 21H1z" fill="#f5c400" stroke="#8a6d00"/><rect x="11" y="8" width="2" height="7" fill="#222"/><rect x="11" y="17" width="2" height="2" fill="#222"/></svg>`;
}

function renderTray() {
  const menu = document.getElementById("tray-menu");
  menu.hidden = !state.trayOpen || !state.running;
  menu.innerHTML = `
    <div class="state">${escape(trayLine())}</div>
    <hr>
    <div class="item default" data-action="open-window">Open Floppy Station</div>
    <div class="item ${state.detection ? "checked" : ""}" data-action="toggle-detection">Detect disks</div>
    <hr>
    <div class="item" data-action="exit">Exit</div>`;
}

function renderControls() {
  const el = document.getElementById("controls");
  const inserted = state.diskId;
  const diskButtons = Object.entries(DISKS)
    .map(([id, d]) => `<button data-action="insert" data-disk="${id}" ${!state.running || !state.driveConnected || inserted ? "disabled" : ""}>Insert: ${escape(d.label)}</button>`)
    .join("");
  el.innerHTML = `
    <h1>Mockup controls</h1>
    <p class="note">Not part of the app. Simulates the drive. Reading takes ${READ_MS / 1000} s here (about 6 s on a real drive).</p>
    <h2>Disk</h2>
    ${diskButtons}
    <button data-action="remove" ${inserted ? "" : "disabled"}>Remove the disk</button>
    <h2>Drive</h2>
    <button data-action="unplug" ${state.driveConnected ? "" : "disabled"}>Unplug the drive</button>
    <button data-action="plug" ${state.driveConnected ? "disabled" : ""}>Plug the drive in${state.unpluggedWith ? " (disk inside)" : ""}</button>
    <h2>App</h2>
    ${state.running ? "" : `<button data-action="start">Start Floppy Station</button>`}
    <div class="field-row"><input type="checkbox" id="ctl-first-run" data-action="first-run" ${state.firstRun ? "checked" : ""}><label for="ctl-first-run">First run</label></div>
    <div class="field-row"><input type="checkbox" id="ctl-banner" data-action="banner" ${state.banner ? "checked" : ""}><label for="ctl-banner">Damaged settings file</label></div>
    <p class="note" style="margin-top:10px">The tray icon is at the bottom right. Closing the window keeps the app running there.</p>`;
}

// ---------------------------------------------------------------------
// Dialogs

function openDialog(title, body, wide = false) {
  const layer = document.getElementById("dialog-layer");
  layer.innerHTML = `<div class="window dialog ${wide ? "wide" : ""}">
    <div class="title-bar"><div class="title-bar-text">${escape(title)}</div>
    <div class="title-bar-controls"><button aria-label="Close" data-action="close-dialog"></button></div></div>
    <div class="window-body">${body}</div></div>`;
  layer.hidden = false;
}

function closeDialog() {
  const layer = document.getElementById("dialog-layer");
  layer.hidden = true;
  layer.innerHTML = "";
}

let pickedProcess = null;

function processDialog() {
  pickedProcess = null;
  const items = OPEN_APPS.map((name) => `<div data-action="pick-process-item" data-name="${escape(name)}"><span>${escape(name)}</span></div>`).join("");
  openDialog("Pick the process to close", `
    <p style="margin-top:0">Open applications:</p>
    <div class="list tall" id="process-list">${items}</div>
    <p class="muted">Tip: start the game first, then pick it here.</p>
    <div class="actions"><button data-action="process-ok">OK</button><button data-action="close-dialog">Cancel</button></div>`);
}

// ---------------------------------------------------------------------
// Actions

function fillFormFrom(config) {
  if (config.kind === "steam") {
    state.form = { ...emptyForm(), kind: "steam", appId: config.appId, name: config.name, process: config.process || "" };
  } else {
    state.form = { ...emptyForm(), kind: "exe", name: config.name, path: config.path, args: (config.args || []).join(" "), process: config.process || "" };
  }
}

function setTab(tab) {
  state.tab = tab;
  if (tab === "disk") {
    const d = disk();
    if (d && d.content === "configured" && !state.writing && !state.writeResult) {
      fillFormFrom(d.config);
    }
  }
}

function write() {
  const config = formConfig();
  const d = disk();
  if (!config || !d) return;
  state.writing = true;
  state.writeResult = null;
  render();
  const id = state.diskId;
  setTimeout(() => {
    if (state.diskId !== id) {
      return;
    }
    state.writing = false;
    if (d.writeProtected) {
      state.writeResult = { ok: false, text: "The disk is write protected. Slide the tab and try again." };
    } else {
      d.content = "configured";
      d.config = config;
      d.missing = false;
      if (config.kind === "exe" && !isApproved(config)) {
        state.approvals.push({ key: approvalKey(config), name: config.name, path: config.path, date: "2026-10-28" });
      }
      state.writeResult = { ok: true, text: "Written to the disk. Remove and insert it to play." };
    }
    render();
  }, WRITE_MS);
}

document.addEventListener("click", (event) => {
  const target = event.target.closest("[data-action]");
  if (!target) {
    if (state.trayOpen && !event.target.closest("#tray-menu")) {
      state.trayOpen = false;
      renderTray();
    }
    return;
  }
  const action = target.dataset.action;
  if (target.tagName === "INPUT" || target.tagName === "SELECT") {
    // Handled on change, except checkboxes in the controls panel.
    if (!["first-run", "banner", "toggle-detection"].includes(action)) return;
  }
  switch (action) {
    case "tab":
      setTab(target.dataset.tab);
      if (!state.windowOpen) state.windowOpen = true;
      break;
    case "insert":
      insertDisk(target.dataset.disk);
      return;
    case "remove":
      removeDisk("ejected");
      return;
    case "unplug":
      unplugDrive();
      return;
    case "plug":
      plugDrive();
      return;
    case "toggle-detection":
      state.detection = !state.detection;
      state.trayOpen = false;
      break;
    case "first-run":
      state.firstRun = target.checked;
      state.tab = "status";
      state.windowOpen = true;
      break;
    case "banner":
      state.banner = target.checked;
      break;
    case "dismiss-banner":
      state.banner = false;
      break;
    case "finish-first-run":
      state.firstRun = false;
      break;
    case "close-window":
      state.windowOpen = false;
      break;
    case "open-window":
      state.windowOpen = true;
      state.trayOpen = false;
      break;
    case "toggle-tray":
      state.trayOpen = !state.trayOpen;
      renderTray();
      event.stopPropagation();
      return;
    case "exit":
      state.running = false;
      state.trayOpen = false;
      hideBox();
      break;
    case "start":
      state.running = true;
      state.windowOpen = false;
      break;
    case "hide-box":
      hideBox();
      return;
    case "box-open":
      hideBox();
      setTab(target.dataset.tab);
      state.windowOpen = true;
      break;
    case "pick-game": {
      if (state.writing || disk().writeProtected) return;
      const game = LIBRARY.find((g) => g.appId === Number(target.dataset.app));
      state.form.appId = game.appId;
      state.form.name = game.name;
      state.writeResult = null;
      break;
    }
    case "browse":
      state.form.path = "C:\\Games\\Doom\\doom.exe";
      if (!state.form.name) state.form.name = "Doom";
      state.writeResult = null;
      break;
    case "pick-process":
      processDialog();
      return;
    case "pick-process-item":
      pickedProcess = target.dataset.name;
      for (const item of document.querySelectorAll("#process-list div")) {
        item.classList.toggle("selected", item === target);
      }
      return;
    case "process-ok":
      if (pickedProcess) state.form.process = pickedProcess;
      closeDialog();
      break;
    case "close-dialog":
      closeDialog();
      return;
    case "preview": {
      const config = formConfig();
      if (!config) return;
      openDialog("Preview: floppy-station.toml", `<pre>${escape(toToml(config))}</pre>
        <p class="muted">This file is written to the root of the disk. Other files on the disk are left alone.</p>
        <div class="actions"><button data-action="close-dialog">Close</button></div>`, true);
      return;
    }
    case "write":
      write();
      return;
    case "remove-config":
      openDialog("Remove configuration", `<p style="margin-top:0">Remove the game from this disk? Other files on the disk are left alone.</p>
        <div class="actions"><button data-action="confirm-remove-config">Remove</button><button data-action="close-dialog">Cancel</button></div>`);
      return;
    case "confirm-remove-config": {
      const d = disk();
      if (d) {
        d.content = "not_configured";
        delete d.config;
        state.form = emptyForm();
        state.writeResult = { ok: true, text: "The configuration was removed from the disk." };
      }
      closeDialog();
      break;
    }
    case "approve": {
      const d = disk();
      state.approvals.push({ key: approvalKey(d.config), name: d.config.name, path: d.config.path, date: "2026-10-28" });
      openDialog("Approved", `<p style="margin-top:0">${escape(d.config.name)} is approved. Remove and insert the disk to launch it.</p>
        <div class="actions"><button data-action="close-dialog">OK</button></div>`);
      break;
    }
    case "not-now":
      setTab("status");
      break;
    case "select-approval":
      state.selectedApproval = Number(target.dataset.index);
      break;
    case "revoke": {
      const entry = state.approvals[state.selectedApproval];
      if (!entry) return;
      openDialog("Revoke approval", `<p style="margin-top:0">Revoke the approval of ${escape(entry.name)}? Its disk won't launch until you approve it again.</p>
        <div class="actions"><button data-action="confirm-revoke">Revoke</button><button data-action="close-dialog">Cancel</button></div>`);
      return;
    }
    case "confirm-revoke":
      state.approvals.splice(state.selectedApproval, 1);
      state.selectedApproval = null;
      closeDialog();
      break;
    default:
      return;
  }
  render();
});

for (const tab of document.querySelectorAll("#tabs [role=tab]")) {
  tab.addEventListener("click", () => {
    setTab(tab.dataset.tab);
    render();
  });
}

// Form fields and preferences update the state as they change.
document.addEventListener("input", (event) => {
  const target = event.target;
  if (target.dataset.action === "form") {
    const field = target.dataset.field;
    state.form[field] = target.value;
    state.writeResult = null;
    if (field === "kind") {
      state.form = { ...emptyForm(), kind: target.value };
      render();
    } else if (field === "search") {
      render();
      const search = document.querySelector('[data-field="search"]');
      search.focus();
      search.setSelectionRange(search.value.length, search.value.length);
    } else {
      // Only refresh what depends on the field, keeping the focus.
      const hint = document.getElementById("process-hint");
      if (hint) hint.innerHTML = processHintHtml();
      const enable = Boolean(formConfig());
      for (const action of ["preview", "write"]) {
        const button = document.querySelector(`[data-action="${action}"]`);
        if (button) button.disabled = !enable;
      }
    }
  } else if (target.dataset.action === "pref") {
    const pref = target.dataset.pref;
    if (target.type === "checkbox") {
      state.prefs[pref] = target.checked;
    } else if (target.type === "number") {
      state.prefs[pref] = Number(target.value);
    } else {
      state.prefs[pref] = target.value;
    }
    if (target.type === "radio" || target.tagName === "SELECT") render();
  }
});

function tick() {
  const now = new Date();
  document.getElementById("clock").textContent = now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

setInterval(tick, 10000);
tick();
render();
