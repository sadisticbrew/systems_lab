<script lang="ts">
  import { tick } from "svelte";
  import { lab } from "../lib/state.svelte";
  import { revealRow } from "../lib/reveal";

  let body = $state<HTMLDivElement>();

  const src = $derived(lab.cur?.source ?? null);
  const lines = $derived(src ? lab.sources[src.file] : undefined);

  $effect(() => {
    if (src) lab.ensureSource(src.file);
  });

  $effect(() => {
    void [src?.line, lines];
    tick().then(() => revealRow(body, body?.querySelector(".cur")));
  });

  function jump(file: string, line: number) {
    void lab.nav("line-next", { file, line });
  }
</script>

<div class="panel">
  <h2>Source {src ? `· ${src.file}:${src.line}` : ""}</h2>
  <div class="body mono" bind:this={body}>
    {#if !src}
      <div class="dim pad">No source line for {lab.cur?.loc ?? "this step"}.</div>
    {:else if !lines}
      <div class="dim pad">loading…</div>
    {:else}
      {#each lines as text, i}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="ln clickable" class:cur={i + 1 === src.line} onclick={() => jump(src.file, i + 1)} title="Jump to the next time line {i + 1} runs">
          <span class="num dim">{i + 1}</span><span class="txt">{text}</span>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .pad { padding: 8px; }
  .ln { display: flex; white-space: pre; }
  .num { min-width: 3.5em; text-align: right; padding-right: 1em; user-select: none; flex: none; }
  .cur, .cur:hover { background: var(--hl); }
</style>
