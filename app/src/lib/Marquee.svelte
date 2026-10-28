<script lang="ts">
  // XP's indeterminate progress bar (styles in app.css).
  let { label }: { label: string } = $props();
</script>

<div class="marquee" role="progressbar" aria-label={label}><span></span></div>
