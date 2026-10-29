<script lang="ts">
  // Shown at the top of the Status tab on the first run: pick the drive,
  // insert a disk, and a word on how it works.
  import { onMount } from "svelte";
  import { api, type AppState, type Settings, type Tab } from "../lib/api";
  import { problemText, t } from "../lib/i18n.svelte";

  let {
    app,
    settings,
    onSaved,
    openTab,
  }: {
    app: AppState;
    settings: Settings;
    onSaved: (settings: Settings) => void;
    openTab: (tab: Tab) => void;
  } = $props();

  let drives: string[] = $state([]);
  let error: string | null = $state(null);

  onMount(async () => {
    drives = await api.removableDrives();
  });

  async function setDrive(drive: string) {
    try {
      await api.saveSettings({ ...settings, drive });
      error = null;
    } catch (code) {
      error = problemText(code);
    }
    onSaved(await api.settings());
  }

  async function done() {
    try {
      await api.finishWelcome();
    } catch (code) {
      error = problemText(code);
    }
  }
</script>

<fieldset>
  <legend>{t("ui.welcome.title")}</legend>
  <p class="intro">{t("ui.welcome.intro")}</p>
  <div class="steps">
    <label for="welcome-drive">{t("ui.welcome.drive")}</label>
    <div>
      <select
        id="welcome-drive"
        value={settings.drive}
        onchange={(event) => setDrive(event.currentTarget.value)}
      >
        {#each drives as drive (drive)}
          <option value={drive}>{drive}</option>
        {/each}
      </select>
    </div>
    <span>{t("ui.welcome.insert")}</span>
    <div class="row">
      {#if app.driveStatus === "missing"}
        <span class="warn">{t("ui.drive.missing", { drive: app.drive })}</span>
      {:else if app.driveStatus === "reading"}
        <span>{t("box.reading")}</span>
      {:else if app.driveStatus !== "present" || !app.disk}
        <span class="muted">{t("ui.welcome.waiting")}</span>
      {:else if app.disk.config}
        <span class="ok">{t("ui.welcome.found", { name: app.disk.config.name })}</span>
      {:else if app.disk.code === "disk_not_configured"}
        <span>{t("disk_not_configured")}</span>
        <button onclick={() => openTab("disk")}>
          {t("ui.button.configure")}
        </button>
      {:else}
        <span class="warn">{t("ui.welcome.unusable")}</span>
      {/if}
    </div>
  </div>
  {#if error}<p class="warn">{error}</p>{/if}
  <div class="actions">
    <button onclick={done}>{t("ui.button.done")}</button>
  </div>
</fieldset>

<style>
  .intro {
    margin-top: 0;
  }

  .steps {
    display: grid;
    grid-template-columns: 170px 1fr;
    gap: 8px;
    align-items: center;
  }
</style>
