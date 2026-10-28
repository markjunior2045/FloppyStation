<script lang="ts">
  // The app's small icons: the XP warning triangle and info circle.
  let { name, size = 12 }: { name: "warn" | "info"; size?: number } = $props();
</script>

<svg class="icon" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
  {#if name === "warn"}
    <path d="M12 2 23 21H1z" fill="#f5c400" stroke="#8a6d00" />
    <rect x="11" y="8" width="2" height="7" fill="#222" />
    <rect x="11" y="17" width="2" height="2" fill="#222" />
  {:else}
    <circle cx="12" cy="12" r="10.5" fill="#2f6fde" stroke="#173f9a" />
    <rect x="11" y="10" width="2" height="8" fill="#fff" />
    <rect x="11" y="6" width="2" height="2" fill="#fff" />
  {/if}
</svg>

<style>
  .icon {
    flex: none;
    vertical-align: -2px;
  }
</style>
