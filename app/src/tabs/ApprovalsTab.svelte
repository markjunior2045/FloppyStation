<script lang="ts">
  // The programs approved on this PC, and the approval of the program on
  // the disk in the drive when it isn't approved yet.
  import { onMount } from "svelte";
  import { api, type AppState, type TrustEntry } from "../lib/api";
  import { problemText, t } from "../lib/i18n.svelte";
  import Icon from "../lib/Icon.svelte";

  let { app }: { app: AppState } = $props();

  let entries = $state<TrustEntry[] | null>(null);
  let selected: string | null = $state(null);
  let confirmingRevoke = $state(false);
  // The disk whose approval the user put off, by config hash.
  let notNow: string | null = $state(null);
  let message: { ok: boolean; text: string } | null = $state(null);

  const config = $derived(app.driveStatus === "present" ? (app.disk?.config ?? null) : null);
  const pending = $derived(
    config && config.kind === "exe" && !config.approved && config.hash !== notNow ? config : null,
  );
  const selectedEntry = $derived(entries?.find((entry) => entry.hash === selected) ?? null);

  onMount(load);

  async function load() {
    try {
      entries = await api.approvals();
    } catch (problem) {
      entries = [];
      message = { ok: false, text: problemText(problem) };
    }
  }

  async function approve() {
    try {
      await api.approveDisk();
      message = { ok: true, text: t("ui.approvals.approved") };
    } catch (problem) {
      message = { ok: false, text: problemText(problem) };
    }
    await load();
  }

  async function revoke() {
    confirmingRevoke = false;
    if (!selected) return;
    try {
      await api.revokeApproval(selected);
      message = null;
      selected = null;
    } catch (problem) {
      message = { ok: false, text: problemText(problem) };
    }
    await load();
  }
</script>

{#if pending}
  <fieldset>
    <legend>{t("ui.approvals.pending", { drive: app.drive })}</legend>
    <div class="title"><Icon name="warn" /> {pending.name}</div>
    <dl class="details">
      <dt>{t("ui.approvals.program")}</dt>
      <dd>{pending.path}</dd>
      <dt>{t("ui.approvals.args")}</dt>
      <dd>{pending.argsLine || t("ui.approvals.none")}</dd>
      <dt>{t("ui.approvals.closes")}</dt>
      <dd>{pending.process ?? t("ui.approvals.itself")}</dd>
    </dl>
    {#if !pending.found}
      <div class="warn note"><Icon name="warn" /> {t("ui.approvals.missing")}</div>
    {/if}
    <div class="row">
      <span class="muted">{t("ui.approvals.trust")}</span>
      <span class="spacer"></span>
      <button onclick={approve}>{t("ui.button.approve")}</button>
      <button onclick={() => (notNow = pending.hash)}>{t("ui.button.not_now")}</button>
    </div>
  </fieldset>
{/if}

{#if message}
  <div class="result" class:ok={message.ok} class:warn={!message.ok}>
    <Icon name={message.ok ? "info" : "warn"} />
    {message.text}
  </div>
{/if}

<p class="list-title">{t("ui.approvals.list")}</p>
{#if entries && entries.length > 0}
  <div class="table-wrap">
    <table class="approvals">
      <colgroup>
        <col style="width: 30%" />
        <col />
        <col style="width: 130px" />
      </colgroup>
      <thead>
        <tr>
          <th>{t("ui.approvals.col.name")}</th>
          <th>{t("ui.approvals.col.program")}</th>
          <th>{t("ui.approvals.col.added")}</th>
        </tr>
      </thead>
      <tbody>
        {#each entries as entry (entry.hash)}
          <tr
            class:selected={selected === entry.hash}
            onclick={() => {
              selected = entry.hash;
              confirmingRevoke = false;
            }}
          >
            <td title={entry.name}>{entry.name}</td>
            <td title={entry.path}>{entry.path}</td>
            <td>{entry.added}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{:else}
  <div class="table-wrap empty muted">
    {entries === null ? t("ui.loading") : t("ui.approvals.empty")}
  </div>
{/if}

<div class="actions">
  {#if confirmingRevoke && selectedEntry}
    <span>{t("ui.approvals.revoke_confirm", { name: selectedEntry.name })}</span>
    <button onclick={revoke}>{t("ui.button.revoke")}</button>
    <button onclick={() => (confirmingRevoke = false)}>{t("ui.button.cancel")}</button>
  {:else}
    <button disabled={!selectedEntry} onclick={() => (confirmingRevoke = true)}>
      {t("ui.button.revoke")}
    </button>
  {/if}
</div>

<style>
  .note {
    margin-bottom: 6px;
  }

  .list-title {
    margin: 8px 0 4px;
  }

  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 20px;
    text-align: center;
  }

  tr {
    cursor: default;
  }

  .result {
    margin: 0 0 4px;
  }
</style>
