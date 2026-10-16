<script lang="ts">
  import { untrack } from "svelte";
  import { api } from "../lib/api";
  import { parseGuide, type Block, type Predict } from "../lib/guide";
  import { lab } from "../lib/state.svelte";
  import GuidePredict from "./GuidePredict.svelte";
  import GuideText from "./GuideText.svelte";

  let blocks = $state.raw<Block[] | null>(null);
  let error = $state<string | null>(null);
  /** Changes with every fetch, so the cards start over and check their stored answers against the new recording. */
  let version = $state(0);
  let loading = 0;

  async function load() {
    const id = ++loading;
    try {
      const g = await api.guide();
      if (id !== loading) return;
      blocks = parseGuide(g.markdown);
      error = null;
      version++;
    } catch (e) {
      if (id !== loading) return;
      blocks = [];
      error = e instanceof Error ? e.message : String(e);
    }
  }

  $effect(() => {
    void lab.status.generation;
    untrack(load);
  });

  const predictions = $derived((blocks ?? []).filter((b): b is Predict => b.kind === "predict"));
  const checked = $derived(predictions.filter((p) => lab.notes.guide?.[p.key]).length);
</script>

{#if predictions.length}
  <div class="progress dim" title="Predictions you have checked">{checked} / {predictions.length} predictions checked</div>
{/if}
<div class="guide">
  {#if error}
    <div class="dim">Could not load the guide: {error}</div>
  {:else if blocks === null}
    <div class="dim">Loading the guide…</div>
  {:else}
    {#key version}
      {#each blocks as b}
        {#if b.kind === "heading"}
          {#if b.level === 1}<h1><GuideText spans={b.spans} /></h1>
          {:else if b.level === 2}<h2><GuideText spans={b.spans} /></h2>
          {:else}<h3><GuideText spans={b.spans} /></h3>{/if}
        {:else if b.kind === "para"}
          <p><GuideText spans={b.spans} /></p>
        {:else if b.kind === "list"}
          <ul>{#each b.items as item}<li><GuideText spans={item} /></li>{/each}</ul>
        {:else if b.kind === "code"}
          <pre class="mono">{b.text}</pre>
        {:else if b.kind === "go"}
          <button class="go" title="Jump to {b.loc}" onclick={() => lab.locate(b.loc)}>Go to {b.loc}</button>
        {:else if b.kind === "predict"}
          <GuidePredict p={b} />
        {:else}
          <div class="bad mono">line {b.line}: {b.error}</div>
        {/if}
      {/each}
    {/key}
  {/if}
</div>

<style>
  .progress { position: sticky; top: 0; z-index: 1; padding: 2px 14px; font-size: 11px; background: var(--panel); border-bottom: 1px solid var(--border); }
  .guide { max-width: 80ch; padding: 4px 14px 14px; }
  h1 { font-size: 16px; margin: 8px 0 6px; }
  h2 { font-size: 14px; margin: 14px 0 4px; }
  h3 { font-size: 13px; margin: 10px 0 4px; }
  p { margin: 6px 0; line-height: 1.5; }
  ul { margin: 6px 0; padding-left: 20px; }
  pre { margin: 6px 0; padding: 6px 8px; overflow: auto; background: var(--bg); border: 1px solid var(--border); border-radius: 4px; }
  .go { margin: 4px 0; color: var(--accent); }
  .bad { margin: 6px 0; color: var(--red); font-size: 12px; }
</style>
