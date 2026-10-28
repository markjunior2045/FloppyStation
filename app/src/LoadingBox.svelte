<script lang="ts">
  // The small box shown in the middle of the screen while a disk is read.
  // The backend decides what it shows and when it closes.
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import icon from "./assets/icon.svg";
  import { t } from "./lib/i18n";

  interface BoxState {
    /** Key of the message in the language file; none while reading. */
    message: string | null;
  }

  // The button each message offers, if any.
  const BUTTONS: Record<string, string> = {
    "box.blank": "box.configure",
    "box.untrusted": "box.review",
    "box.failed": "box.details",
    "box.unusable": "box.details",
    "box.newer": "box.details",
  };

  // Messages that are warnings rather than information.
  const WARNINGS = new Set(["box.untrusted", "box.missing", "box.failed", "box.unusable"]);

  let state: BoxState | null = $state({ message: null });

  onMount(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      unlisten = await listen<BoxState>("loading-box", (event) => (state = event.payload));
      state = await invoke<BoxState | null>("loading_box_state");
    })();
    return () => unlisten?.();
  });

  const close = () => invoke("close_loading_box");
  const openMain = () => invoke("open_main_window");
</script>

<div class="window box">
  <div class="title-bar">
    <div class="title-bar-text"><img src={icon} alt="" />{t("ui.title")}</div>
    {#if state?.message}
      <div class="title-bar-controls">
        <button aria-label="Close" onclick={close}></button>
      </div>
    {/if}
  </div>
  <div class="window-body">
    {#if state?.message}
      <div class="message">
        {#if WARNINGS.has(state.message)}
          <svg width="24" height="24" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M12 2 23 21H1z" fill="#f5c400" stroke="#8a6d00" />
            <rect x="11" y="8" width="2" height="7" fill="#222" />
            <rect x="11" y="17" width="2" height="2" fill="#222" />
          </svg>
        {:else}
          <svg width="24" height="24" viewBox="0 0 24 24" aria-hidden="true">
            <circle cx="12" cy="12" r="10.5" fill="#2f6fde" stroke="#173f9a" />
            <rect x="11" y="10" width="2" height="8" fill="#fff" />
            <rect x="11" y="6" width="2" height="2" fill="#fff" />
          </svg>
        {/if}
        <span>{t(state.message)}</span>
      </div>
      {#if BUTTONS[state.message]}
        <div class="actions">
          <button onclick={openMain}>{t(BUTTONS[state.message])}</button>
        </div>
      {/if}
    {:else}
      <p class="reading">{t("box.reading")}</p>
      <div class="marquee" role="progressbar" aria-label={t("box.reading")}><span></span></div>
    {/if}
  </div>
</div>

<style>
  .box {
    height: 100%;
    box-sizing: border-box;
  }

  .window-body {
    margin: 12px 14px;
  }

  .reading {
    margin: 0 0 8px;
  }

  .message {
    display: flex;
    gap: 8px;
    align-items: flex-start;
  }

  .message svg {
    flex: none;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 6px;
  }
</style>
