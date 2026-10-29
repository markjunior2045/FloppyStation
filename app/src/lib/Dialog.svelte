<script lang="ts">
  // A small XP dialog over the main window (the process picker, the preview).
  import type { Snippet } from "svelte";
  import { t } from "./i18n.svelte";

  let {
    title,
    wide = false,
    onclose,
    children,
  }: { title: string; wide?: boolean; onclose: () => void; children: Snippet } = $props();
</script>

<svelte:window onkeydown={(event) => event.key === "Escape" && onclose()} />

<div class="dialog-layer">
  <div class="window dialog" class:wide role="dialog" aria-label={title}>
    <div class="title-bar">
      <div class="title-bar-text">{title}</div>
      <div class="title-bar-controls">
        <button aria-label={t("ui.button.close")} onclick={onclose}></button>
      </div>
    </div>
    <div class="window-body">
      {@render children()}
    </div>
  </div>
</div>
