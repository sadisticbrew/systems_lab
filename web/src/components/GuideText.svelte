<script lang="ts">
  import type { Span } from "../lib/guide";

  let { spans }: { spans: Span[] } = $props();
</script>

{#each spans as s}{#if s.code}<code class:bold={s.bold}>{s.text}</code>{:else if s.bold}<b>{s.text}</b>{:else}{s.text}{/if}{/each}

<style>
  code { font-family: var(--mono); font-size: 0.95em; background: var(--bg); border: 1px solid var(--border); border-radius: 3px; padding: 0 3px; }
  code.bold { font-weight: 700; }
</style>
