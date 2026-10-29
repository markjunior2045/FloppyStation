<script lang="ts">
  // The main window: the XP frame with the Status, Disk, Approvals and
  // Preferences tabs. The backend sends the current state on every change.
  import { listen } from "@tauri-apps/api/event";
  import { onMount, tick } from "svelte";
  import { api, type AppEvent, type AppState, type Settings, type Tab } from "./lib/api";
  import { t } from "./lib/i18n";
  import TitleBar from "./lib/TitleBar.svelte";
  import ApprovalsTab from "./tabs/ApprovalsTab.svelte";
  import DiskTab from "./tabs/DiskTab.svelte";
  import PreferencesTab from "./tabs/PreferencesTab.svelte";
  import StatusTab from "./tabs/StatusTab.svelte";

  const TABS: Tab[] = ["status", "disk", "approvals", "prefs"];
  const MAX_EVENTS = 200;

  let tab: Tab = $state("status");
  let appState: AppState | null = $state(null);
  let settings: Settings | null = $state(null);
  let events: AppEvent[] = $state([]);
  let version = $state("");
  let panel: HTMLElement | undefined = $state();

  onMount(() => {
    const unlisten: (() => void)[] = [];
    (async () => {
      // Listen first, then fetch what happened before the window opened;
      // `seq` drops the events received both ways.
      unlisten.push(await listen<AppState>("app-state", (event) => (appState = event.payload)));
      unlisten.push(await listen<AppEvent>("app-event", (event) => addEvents([event.payload])));
      // The loading box's buttons open the window on a given tab.
      unlisten.push(await listen<Tab>("open-tab", (event) => openTab(event.payload)));
      const asked = await api.takeMainTab();
      if (asked) openTab(asked);
      addEvents(await api.recentEvents());
      appState = await api.state();
      settings = await api.settings();
      version = await api.version();
    })();
    return () => unlisten.forEach((stop) => stop());
  });

  function addEvents(incoming: AppEvent[]) {
    const known = new Set(events.map((event) => event.seq));
    const fresh = incoming.filter((event) => !known.has(event.seq));
    if (fresh.length === 0) return;
    events = [...events, ...fresh].sort((a, b) => a.seq - b.seq).slice(-MAX_EVENTS);
    // Detection or the drive may have changed from the tray.
    if (fresh.some((event) => event.code.startsWith("detection_"))) {
      api.settings().then((loaded) => (settings = loaded));
    }
  }

  async function openTab(next: Tab) {
    tab = next;
    await tick();
    panel?.scrollTo({ top: 0 });
  }
</script>

<div class="window app-window">
  <TitleBar title={t("ui.title")} />
  <div class="window-body">
    <menu role="tablist">
      {#each TABS as name (name)}
        <button role="tab" aria-selected={tab === name} onclick={() => openTab(name)}>
          {t(`ui.tab.${name}`)}
        </button>
      {/each}
    </menu>
    <div role="tabpanel" bind:this={panel}>
      {#if !appState || !settings}
        <div class="muted">{t("ui.loading")}</div>
      {:else if tab === "status"}
        <StatusTab app={appState} {settings} {events} {openTab} onSaved={(saved) => (settings = saved)} />
      {:else if tab === "disk"}
        <DiskTab app={appState} />
      {:else if tab === "approvals"}
        <ApprovalsTab app={appState} />
      {:else}
        <PreferencesTab {settings} detection={appState.detection} onSaved={(saved) => (settings = saved)} />
      {/if}
    </div>
  </div>
  <div class="status-bar">
    <p class="status-bar-field">{appState?.line ?? ""}</p>
    <p class="status-bar-field">
      {appState ? t(appState.detection ? "ui.status.detection_on" : "ui.status.detection_off") : ""}
    </p>
    <p class="status-bar-field">{version ? t("ui.status.version", { version }) : ""}</p>
  </div>
</div>

<style>
  [role="tabpanel"] {
    flex: 1;
    overflow: auto;
    margin-bottom: 0;
  }
</style>
