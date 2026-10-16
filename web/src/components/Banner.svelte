<script lang="ts">
  import { lab } from "../lib/state.svelte";

  const source = $derived(lab.info?.source_path ?? null);
  const name = $derived(source?.split("/").pop() ?? "the program");
  const failed = $derived(lab.info?.mode === "run" ? "Build failed" : "Reload failed");
</script>

{#if lab.status.building}
  <div class="banner info" role="status" title={source ?? undefined}>Rebuilding and re-recording {name}…</div>
{:else if lab.status.error}
  <div class="banner err" role="alert">
    <details open>
      <summary>{failed} — still showing the previous recording</summary>
      <pre class="mono">{lab.status.error}</pre>
    </details>
  </div>
{/if}

<style>
  .banner { padding: 4px 10px; border-radius: 4px; border: 1px solid var(--border); }
  .info { background: var(--cur); border-color: var(--accent); }
  .err { background: var(--panel); border-color: var(--red); }
  summary { cursor: pointer; color: var(--red); font-weight: 600; }
  pre { margin: 4px 0 0; max-height: 28vh; overflow: auto; white-space: pre-wrap; font-size: 12px; }
</style>
