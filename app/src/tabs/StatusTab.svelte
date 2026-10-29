<script lang="ts">
  // What is going on now: the drive, the disk in it and the game.
  import icon from "../assets/icon.svg";
  import { api, type AppEvent, type AppState, type Settings, type Tab } from "../lib/api";
  import { t } from "../lib/i18n";
  import Icon from "../lib/Icon.svelte";
  import Marquee from "../lib/Marquee.svelte";
  import Welcome from "./Welcome.svelte";

  let {
    app,
    settings,
    events,
    openTab,
    onSaved,
  }: {
    app: AppState;
    settings: Settings;
    events: AppEvent[];
    openTab: (tab: Tab) => void;
    onSaved: (settings: Settings) => void;
  } = $props();

  let bannerClosed = $state(false);
  let activity: HTMLElement | undefined = $state();

  const disk = $derived(app.disk);
  const config = $derived(disk?.config ?? null);
  // Oldest first, like a log; the box keeps the newest in view.
  const recent = $derived(events.slice(-50));

  $effect(() => {
    if (activity && recent.length) activity.scrollTop = activity.scrollHeight;
  });

  function target(): string {
    if (!config) return "";
    if (config.kind === "steam") return t("ui.disk.steam", { appId: config.appId ?? "" });
    const args = config.args.length ? ` ${config.args.join(" ")}` : "";
    return t("ui.disk.program", { path: `${config.path}${args}` });
  }

  function closes(): string {
    if (!config) return "";
    if (config.process) return t("ui.disk.closes", { process: config.process });
    return t(config.kind === "steam" ? "ui.disk.not_closed" : "ui.disk.closes_itself");
  }

  function removal(): string {
    if (settings.on_removal === "close") {
      return t("ui.game.removal.close", { timeout: settings.close_timeout_s });
    }
    return t(settings.on_removal === "force" ? "ui.game.removal.force" : "ui.game.removal.none");
  }

  function time(at: number): string {
    return new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  }
</script>

{#if app.settingsError && !bannerClosed}
  <div class="banner">
    <span><Icon name="warn" /> {t(app.settingsError, { detail: "" })}</span>
    <button onclick={() => (bannerClosed = true)}>{t("ui.button.close")}</button>
  </div>
{/if}

{#if app.firstRun}
  <Welcome {app} {settings} {onSaved} {openTab} />
{/if}

<fieldset>
  <legend>{t("ui.status.drive")}</legend>
  <div class="row">
    <span class="drive-icon" class:missing={app.driveStatus === "missing"}></span>
    {#if app.driveStatus === "missing"}
      <div>
        <div class="title warn">{t("ui.drive.missing", { drive: app.drive })}</div>
        <div>{t("ui.drive.missing_hint")}</div>
      </div>
      <span class="spacer"></span>
      <button onclick={() => openTab("prefs")}>{t("ui.button.preferences")}</button>
    {:else}
      <div>
        <div class="title">{app.drive}</div>
        <div>{t(`ui.drive.${app.driveStatus}`)}</div>
      </div>
    {/if}
  </div>
</fieldset>

<fieldset>
  <legend>{t("ui.status.disk")}</legend>
  {#if app.driveStatus === "reading"}
    <div class="reading">{t("box.reading")}</div>
    <Marquee label={t("box.reading")} />
  {:else if !disk}
    <div class="muted">{t(app.driveStatus === "off" ? "ui.disk.off" : "ui.disk.none")}</div>
  {:else if config && !config.approved}
    <div class="row">
      <img class="big-icon" src={icon} alt="" />
      <div>
        <div class="title"><Icon name="warn" /> {config.name}</div>
        <div>{t("ui.disk.untrusted")}</div>
      </div>
      <span class="spacer"></span>
      <button onclick={() => openTab("approvals")}>{t("ui.button.review")}</button>
    </div>
  {:else if config}
    <div class="row">
      <img class="big-icon" src={icon} alt="" />
      <div>
        <div class="title">{config.name}</div>
        <div>{target()} - {closes()}</div>
      </div>
      <span class="spacer"></span>
      <button onclick={() => openTab("disk")}>
        {t("ui.button.edit")}
      </button>
    </div>
  {:else if disk.code === "disk_not_configured"}
    <div class="row">
      <img class="big-icon" src={icon} alt="" />
      <div>{t("disk_not_configured")}</div>
      <span class="spacer"></span>
      <button onclick={() => openTab("disk")}>
        {t("ui.button.configure")}
      </button>
    </div>
  {:else}
    <div class="row">
      <img class="big-icon faded" src={icon} alt="" />
      <div>
        <div class="title warn">{t(disk.code, disk.params)}</div>
        {#if disk.code === "disk_unreadable" || disk.code === "disk_unformatted"}
          <div>{t("ui.disk.format_hint")}</div>
        {/if}
      </div>
    </div>
  {/if}
  {#if disk?.writeProtected}
    <div class="muted note">{t("ui.disk.write_protected")}</div>
  {/if}
</fieldset>

<fieldset>
  <legend>{t("ui.status.game")}</legend>
  {#if app.launched}
    <div class="title ok">{t("game_launched", { name: app.launched })}</div>
    <div>{config && config.kind === "steam" && !config.process ? t("ui.game.no_process") : removal()}</div>
  {:else if app.game}
    <div>{t(app.game.code, app.game.params)}</div>
  {:else}
    <div class="muted">{t("ui.game.none")}</div>
  {/if}
</fieldset>

<div class="row spread">
  <div class="field-row">
    <input
      type="checkbox"
      id="status-detect"
      checked={app.detection}
      onchange={(event) => api.setDetection(event.currentTarget.checked)}
    />
    <label for="status-detect">{t("ui.detect")}</label>
  </div>
</div>

<p class="activity-title">{t("ui.activity")}</p>
<div class="activity" bind:this={activity}>
  {#each recent as event (event.seq)}
    <div class="event">
      <span class="muted">{time(event.at)}</span>
      <span>{t(event.code, event.params)}</span>
    </div>
  {:else}
    <div class="muted">{t("ui.events.empty")}</div>
  {/each}
</div>

<style>
  .reading {
    margin-bottom: 6px;
  }

  .note {
    margin-top: 4px;
  }

  .faded {
    opacity: 0.5;
  }

  .activity-title {
    margin: 10px 0 4px;
  }

  .activity {
    background: #fff;
    border: 1px solid #7f9db9;
    padding: 3px 6px;
    height: 74px;
    overflow: auto;
  }

  .event {
    display: flex;
    gap: 10px;
    white-space: nowrap;
  }
</style>
