<script lang="ts">
  // The preferences. Every change is saved and applied right away.
  import { onMount } from "svelte";
  import { api, type Settings } from "../lib/api";
  import { t } from "../lib/i18n";
  import Icon from "../lib/Icon.svelte";

  let {
    settings,
    detection,
    onSaved,
  }: {
    settings: Settings;
    detection: boolean;
    onSaved: (settings: Settings) => void;
  } = $props();

  let drives: string[] = $state([]);
  let error: string | null = $state(null);

  onMount(async () => {
    drives = await api.removableDrives();
  });

  async function save(change: Partial<Settings>) {
    const updated = { ...settings, ...change };
    try {
      await api.saveSettings(updated);
      error = null;
    } catch (code) {
      error = String(code);
    }
    // The backend clamps out-of-range values: show what it kept.
    onSaved(await api.settings());
  }

  function number(event: Event): number {
    return Number((event.currentTarget as HTMLInputElement).value);
  }
</script>

{#if error}
  <div class="banner">
    <span><Icon name="warn" /> {t("ui.prefs.save_failed", { error: t(error, { detail: "" }) })}</span>
  </div>
{/if}

<div class="prefs">
  <div>
    <fieldset>
      <legend>{t("ui.prefs.drive")}</legend>
      <div class="field-row">
        <label for="drive">{t("ui.prefs.drive_label")}</label>
        <select id="drive" value={settings.drive} onchange={(event) => save({ drive: event.currentTarget.value })}>
          {#each drives as drive (drive)}
            <option value={drive}>{drive}</option>
          {/each}
        </select>
      </div>
      <p class="muted hint">{t("ui.prefs.drive_hint")}</p>
    </fieldset>

    <fieldset>
      <legend>{t("ui.prefs.detection")}</legend>
      <div class="field-row">
        <input
          type="checkbox"
          id="pref-detect"
          checked={detection}
          onchange={(event) => api.setDetection(event.currentTarget.checked)}
        />
        <label for="pref-detect">{t("ui.detect")}</label>
      </div>
      <p class="label">{t("ui.prefs.check_every")}</p>
      <div class="field-row indent">
        <input
          type="number"
          id="poll"
          min="250"
          max="10000"
          step="250"
          value={settings.poll_interval_ms}
          onchange={(event) => save({ poll_interval_ms: number(event) })}
        />
        <label for="poll">{t("ui.prefs.ms_disk")}</label>
      </div>
      <div class="field-row indent">
        <input
          type="number"
          id="empty-poll"
          min="250"
          max="10000"
          step="250"
          value={settings.empty_poll_interval_ms}
          onchange={(event) => save({ empty_poll_interval_ms: number(event) })}
        />
        <label for="empty-poll">{t("ui.prefs.ms_empty")}</label>
      </div>
      <p class="muted hint indent">{t("ui.prefs.click_hint")}</p>
    </fieldset>
  </div>

  <div>
    <fieldset>
      <legend>{t("ui.prefs.removal")}</legend>
      <div class="field-row">
        <input
          type="radio"
          id="removal-close"
          name="removal"
          checked={settings.on_removal === "close"}
          onchange={() => save({ on_removal: "close" })}
        />
        <label for="removal-close">{t("ui.prefs.removal.close")}</label>
        <input
          type="number"
          class="seconds"
          id="timeout"
          min="1"
          max="60"
          value={settings.close_timeout_s}
          onchange={(event) => save({ close_timeout_s: number(event) })}
        />
        <label for="timeout">{t("ui.prefs.seconds")}</label>
      </div>
      <div class="field-row">
        <input
          type="radio"
          id="removal-force"
          name="removal"
          checked={settings.on_removal === "force"}
          onchange={() => save({ on_removal: "force" })}
        />
        <label for="removal-force">{t("ui.prefs.removal.force")}</label>
      </div>
      <div class="field-row">
        <input
          type="radio"
          id="removal-none"
          name="removal"
          checked={settings.on_removal === "none"}
          onchange={() => save({ on_removal: "none" })}
        />
        <label for="removal-none">{t("ui.prefs.removal.none")}</label>
      </div>
    </fieldset>

    <fieldset>
      <legend>{t("ui.prefs.unplug")}</legend>
      <div class="field-row">
        <input
          type="radio"
          id="unplug-removal"
          name="unplug"
          checked={settings.on_drive_disconnect === "removal"}
          onchange={() => save({ on_drive_disconnect: "removal" })}
        />
        <label for="unplug-removal">{t("ui.prefs.unplug.removal")}</label>
      </div>
      <div class="field-row">
        <input
          type="radio"
          id="unplug-ignore"
          name="unplug"
          checked={settings.on_drive_disconnect === "ignore"}
          onchange={() => save({ on_drive_disconnect: "ignore" })}
        />
        <label for="unplug-ignore">{t("ui.prefs.unplug.ignore")}</label>
      </div>
    </fieldset>

    <fieldset>
      <legend>{t("ui.prefs.display")}</legend>
      <div class="field-row">
        <input
          type="checkbox"
          id="pref-box"
          checked={settings.show_loading_box}
          onchange={(event) => save({ show_loading_box: event.currentTarget.checked })}
        />
        <label for="pref-box">{t("ui.prefs.loading_box")}</label>
      </div>
      <div class="field-row">
        <input type="checkbox" id="pref-startup" disabled />
        <label for="pref-startup">{t("ui.prefs.startup")} <span class="muted">{t("ui.prefs.startup_later")}</span></label>
      </div>
    </fieldset>
  </div>
</div>

<style>
  .prefs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0 10px;
  }

  input[type="number"] {
    width: 56px;
  }

  input.seconds {
    width: 40px;
  }

  .label {
    margin: 8px 0 4px;
  }

  .hint {
    margin: 4px 0 0;
  }

  .indent {
    margin-left: 20px;
  }
</style>
