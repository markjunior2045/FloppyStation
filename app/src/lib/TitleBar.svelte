<script lang="ts">
  // The XP title bar. The window has no native frame, so this bar moves the
  // window (drag region) and holds its buttons.
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import icon from "../assets/icon.svg";

  let { title }: { title: string } = $props();

  const appWindow = getCurrentWindow();
</script>

<div class="title-bar" data-tauri-drag-region>
  <div class="title-bar-text" data-tauri-drag-region>
    <img src={icon} alt="" />{title}
  </div>
  <div class="title-bar-controls">
    <button aria-label="Minimize" onclick={() => appWindow.minimize()}></button>
    <button aria-label="Close" onclick={() => appWindow.close()}></button>
  </div>
</div>
