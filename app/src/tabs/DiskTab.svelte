<script lang="ts">
  // Configures the disk in the drive: the form, writing it to the disk and
  // removing the configuration.
  import { onMount } from "svelte";
  import icon from "../assets/icon.svg";
  import { api, type AppState, type LibraryGame } from "../lib/api";
  import Dialog from "../lib/Dialog.svelte";
  import { fillForm, form, formValues } from "../lib/diskForm.svelte";
  import { problemText, t } from "../lib/i18n";
  import Icon from "../lib/Icon.svelte";
  import Marquee from "../lib/Marquee.svelte";
  import ProcessPicker from "../lib/ProcessPicker.svelte";

  let { app }: { app: AppState } = $props();

  // Disk contents a configuration can be written over.
  const CONFIGURABLE = ["disk_configured", "disk_not_configured", "disk_config_invalid"];

  let games: LibraryGame[] | null = $state(null);
  let busy: "writing" | "removing" | null = $state(null);
  let result: { ok: boolean; text: string } | null = $state(null);
  let preview: string | null = $state(null);
  let picking = $state(false);
  let confirmingRemove = $state(false);

  const disk = $derived(app.driveStatus === "present" ? app.disk : null);
  const config = $derived(disk?.config ?? null);
  const configurable = $derived(disk !== null && CONFIGURABLE.includes(disk.code));
  const diskKey = $derived(disk ? (config?.hash ?? disk.code) : null);
  const disabled = $derived(busy !== null || disk?.writeProtected === true);
  const complete = $derived(
    form.name.trim() !== "" &&
      (form.kind === "steam" ? (form.appId ?? 0) > 0 : form.path.trim() !== ""),
  );
  const matches = $derived.by(() => {
    const search = form.search.trim().toLowerCase();
    return (games ?? []).filter(
      (game) => game.name.toLowerCase().includes(search) || String(game.appId).includes(search),
    );
  });

  onMount(async () => {
    games = await api.steamGames();
  });

  // A new disk (or a new configuration on it) fills the form again.
  $effect(() => {
    if (configurable && diskKey && form.diskKey !== diskKey && busy === null) {
      fillForm(config, diskKey);
    }
  });

  // Messages belong to the disk they were about.
  $effect(() => {
    if (!disk) {
      result = null;
      confirmingRemove = false;
    }
  });

  function pickGame(game: LibraryGame) {
    const previous = games?.find((known) => known.appId === form.appId)?.name;
    if (!form.name.trim() || form.name === previous) form.name = game.name;
    form.appId = game.appId;
  }

  async function browse() {
    const path = await api.pickProgram();
    if (!path) return;
    form.path = path;
    if (!form.name.trim()) {
      form.name = path.split("\\").pop()?.replace(/\.exe$/i, "") ?? "";
    }
  }

  async function showPreview() {
    result = null;
    try {
      preview = await api.configPreview(formValues());
    } catch (problem) {
      result = { ok: false, text: problemText(problem) };
    }
  }

  async function write() {
    busy = "writing";
    result = null;
    try {
      await api.writeConfig(formValues());
      result = { ok: true, text: t("ui.form.written") };
    } catch (problem) {
      result = { ok: false, text: problemText(problem) };
    } finally {
      busy = null;
    }
  }

  async function remove() {
    confirmingRemove = false;
    busy = "removing";
    result = null;
    try {
      await api.removeConfig();
      result = { ok: true, text: t("disk_config_removed", { drive: app.drive }) };
    } catch (problem) {
      result = { ok: false, text: problemText(problem) };
    } finally {
      busy = null;
    }
  }

  function summary(): string {
    if (!disk) return "";
    if (!config) return t(disk.code, { detail: "", ...disk.params });
    if (config.kind === "steam") return t("ui.disk.steam", { appId: config.appId ?? "" });
    return t("ui.disk.program", { path: config.path ?? "" });
  }
</script>

{#if !disk}
  <div class="empty">
    <img class="big-icon" src={icon} alt="" />
    <span>{t(app.driveStatus === "reading" ? "box.reading" : "ui.disk.insert")}</span>
  </div>
{:else}
  <fieldset>
    <legend>{t("ui.disk.in_drive", { drive: app.drive })}</legend>
    <div class="row">
      <div>
        {#if config}<div class="title">{config.name}</div>{/if}
        <div class:warn={!configurable}>{summary()}</div>
      </div>
      <span class="spacer"></span>
      {#if config && !confirmingRemove}
        <button {disabled} onclick={() => (confirmingRemove = true)}>{t("ui.button.remove_config")}</button>
      {/if}
    </div>
    {#if confirmingRemove}
      <div class="actions">
        <span>{t("ui.disk.remove_confirm")}</span>
        <button onclick={remove}>{t("ui.button.remove")}</button>
        <button onclick={() => (confirmingRemove = false)}>{t("ui.button.cancel")}</button>
      </div>
    {/if}
    {#if disk.writeProtected}
      <div class="muted note"><Icon name="warn" /> {t("ui.disk.write_protected")}</div>
    {:else if !configurable}
      <div class="muted note">{t("ui.disk.cant_configure")}</div>
    {/if}
  </fieldset>

  {#if configurable}
    <fieldset>
      <legend>{t("ui.form.game")}</legend>
      <div class="form-grid">
        <label for="kind">{t("ui.form.type")}</label>
        <select id="kind" bind:value={form.kind} {disabled}>
          <option value="steam">{t("ui.form.kind.steam")}</option>
          <option value="exe">{t("ui.form.kind.exe")}</option>
        </select>

        {#if form.kind === "steam"}
          <label for="search">{t("ui.form.game_label")}</label>
          <input
            id="search"
            type="text"
            placeholder={t("ui.form.search")}
            bind:value={form.search}
            {disabled}
          />
          <span></span>
          <div class="list" role="listbox" aria-label={t("ui.form.game_label")}>
            {#if games === null}
              <div class="muted">{t("ui.form.library_loading")}</div>
            {:else if games.length === 0}
              <div class="muted">{t("ui.form.library_empty")}</div>
            {:else}
              {#each matches as game (game.appId)}
                <div
                  class="item"
                  class:selected={form.appId === game.appId}
                  role="option"
                  aria-selected={form.appId === game.appId}
                  tabindex="-1"
                  onclick={() => !disabled && pickGame(game)}
                  onkeydown={(event) => event.key === "Enter" && !disabled && pickGame(game)}
                >
                  <span>{game.name}</span><span>{game.appId}</span>
                </div>
              {:else}
                <div class="muted">{t("ui.form.no_match")}</div>
              {/each}
            {/if}
          </div>
          <label for="app-id">{t("ui.form.app_id")}</label>
          <div class="with-button">
            <input id="app-id" type="number" min="1" class="app-id" bind:value={form.appId} {disabled} />
            <span class="muted hint">{t("ui.form.app_id_hint")}</span>
          </div>
        {:else}
          <label for="path">{t("ui.form.program")}</label>
          <div class="with-button">
            <input id="path" type="text" bind:value={form.path} {disabled} />
            <button {disabled} onclick={browse}>{t("ui.button.browse")}</button>
          </div>
          <label for="args">{t("ui.form.args")}</label>
          <input id="args" type="text" bind:value={form.args} {disabled} />
        {/if}

        <label for="name">{t("ui.form.name")}</label>
        <input id="name" type="text" maxlength="100" bind:value={form.name} {disabled} />

        <label for="process">{t("ui.form.close")}</label>
        <div class="with-button">
          <input
            id="process"
            type="text"
            placeholder={t("ui.form.process_placeholder")}
            bind:value={form.process}
            {disabled}
          />
          <button {disabled} onclick={() => (picking = true)}>{t("ui.button.pick")}</button>
        </div>
        {#if !form.process.trim()}
          <span></span>
          {#if form.kind === "steam"}
            <span class="warn hint"><Icon name="warn" /> {t("ui.form.steam_not_closed")}</span>
          {:else}
            <span class="muted hint">{t("ui.form.closes_itself")}</span>
          {/if}
        {/if}
      </div>
    </fieldset>

    <div class="row spread">
      <button disabled={disabled || !complete} onclick={showPreview}>{t("ui.button.preview")}</button>
      <button disabled={disabled || !complete} onclick={write}>{t("ui.button.write")}</button>
    </div>
  {/if}

  {#if busy}
    <div class="busy">{t(busy === "writing" ? "ui.form.writing" : "ui.form.removing")}</div>
    <Marquee label={t(busy === "writing" ? "ui.form.writing" : "ui.form.removing")} />
  {/if}
  {#if result}
    <div class="result" class:ok={result.ok} class:warn={!result.ok}>
      <Icon name={result.ok ? "info" : "warn"} />
      {result.text}
    </div>
  {/if}
{/if}

{#if picking}
  <ProcessPicker
    onpick={(name) => {
      form.process = name;
      picking = false;
    }}
    onclose={() => (picking = false)}
  />
{/if}

{#if preview !== null}
  <Dialog title={t("ui.form.preview_title")} wide onclose={() => (preview = null)}>
    <pre>{preview}</pre>
    <div class="actions">
      <button onclick={() => (preview = null)}>{t("ui.button.ok")}</button>
    </div>
  </Dialog>
{/if}

<style>
  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    height: 100%;
  }

  .note {
    margin-top: 4px;
  }

  .app-id {
    width: 90px !important;
    flex: none !important;
  }

  .hint {
    align-self: center;
  }

  .busy {
    margin: 8px 0 4px;
  }

  .result.ok {
    color: inherit;
  }
</style>
