<script lang="ts">
  // Phase 5 shell: the XP window with the list of what the app does, to show
  // the logic running inside Tauri. The tabs come in phase 7.
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, tick } from "svelte";
  import type { AppEvent, AppInfo } from "./lib/events";
  import { t } from "./lib/i18n";
  import TitleBar from "./lib/TitleBar.svelte";

  const MAX_EVENTS = 200;

  let events: AppEvent[] = $state([]);
  let info: AppInfo | null = $state(null);
  let list: HTMLElement | undefined = $state();

  onMount(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      // Listen first, then fetch what happened before the window opened;
      // `seq` drops the events received both ways.
      unlisten = await listen<AppEvent>("app-event", (event) => add([event.payload]));
      add(await invoke<AppEvent[]>("recent_events"));
      info = await invoke<AppInfo>("app_info");
    })();
    return () => unlisten?.();
  });

  async function add(incoming: AppEvent[]) {
    const known = new Set(events.map((event) => event.seq));
    const fresh = incoming.filter((event) => !known.has(event.seq));
    if (fresh.length === 0) return;
    events = [...events, ...fresh].sort((a, b) => a.seq - b.seq).slice(-MAX_EVENTS);
    await tick();
    list?.scrollTo({ top: list.scrollHeight });
  }

  function time(at: number): string {
    return new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  }
</script>

<div class="window app-window">
  <TitleBar title={t("ui.title")} />
  <div class="window-body">
    <p class="intro">{t("ui.events.intro")}</p>
    <div class="events" bind:this={list}>
      {#each events as event (event.seq)}
        <div class="event">
          <span class="time muted">{time(event.at)}</span>
          <span>{t(event.code, event.params)}</span>
        </div>
      {:else}
        <div class="muted">{t("ui.events.empty")}</div>
      {/each}
    </div>
  </div>
  <div class="status-bar">
    <p class="status-bar-field">{info ? t("ui.status.watching", { drive: info.drive }) : ""}</p>
    <p class="status-bar-field">{info ? t("ui.status.version", { version: info.version }) : ""}</p>
  </div>
</div>

<style>
  .intro {
    margin: 0 0 6px;
  }

  .events {
    flex: 1;
    overflow: auto;
    background: #fff;
    border: 1px solid #7f9db9;
    padding: 4px 6px;
  }

  .event {
    display: flex;
    gap: 10px;
    padding: 1px 0;
  }

  .time {
    flex: none;
    font-variant-numeric: tabular-nums;
  }
</style>
