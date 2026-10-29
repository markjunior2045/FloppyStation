<script lang="ts">
  // Picks the process to close from the programs with an open window. The
  // list is read when this opens, never in the background.
  import { onMount } from "svelte";
  import { api } from "./api";
  import Dialog from "./Dialog.svelte";
  import { t } from "./i18n.svelte";

  let { onpick, onclose }: { onpick: (name: string) => void; onclose: () => void } = $props();

  let names: string[] | null = $state(null);
  let selected: string | null = $state(null);

  onMount(async () => {
    names = await api.openApplications();
  });

  function pick() {
    if (selected) onpick(selected);
  }
</script>

<Dialog title={t("ui.picker.title")} {onclose}>
  <p class="top">{t("ui.picker.list")}</p>
  <div class="list tall" role="listbox" aria-label={t("ui.picker.list")}>
    {#if names === null}
      <div class="muted">{t("ui.picker.loading")}</div>
    {:else}
      {#each names as name (name)}
        <div
          class="item"
          class:selected={selected === name}
          role="option"
          aria-selected={selected === name}
          tabindex="-1"
          onclick={() => (selected = name)}
          ondblclick={() => onpick(name)}
          onkeydown={(event) => event.key === "Enter" && onpick(name)}
        >
          <span>{name}</span>
        </div>
      {:else}
        <div class="muted">{t("ui.picker.empty")}</div>
      {/each}
    {/if}
  </div>
  <p class="muted">{t("ui.picker.tip")}</p>
  <div class="actions">
    <button disabled={!selected} onclick={pick}>{t("ui.button.ok")}</button>
    <button onclick={onclose}>{t("ui.button.cancel")}</button>
  </div>
</Dialog>

<style>
  .top {
    margin-top: 0;
  }
</style>
