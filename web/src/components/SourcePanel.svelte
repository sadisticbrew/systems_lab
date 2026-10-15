<script lang="ts">
  import { tick } from "svelte";
  import { api } from "../lib/api";
  import { lab } from "../lib/state.svelte";

  let cache = $state<Record<string, string[]>>({});
  const pending = new Set<string>();
  let body: HTMLDivElement;

  const src = $derived(lab.cur?.source ?? null);
  const lines = $derived(src ? cache[src.file] : undefined);

  $effect(() => {
    const f = src?.file;
    if (!f || cache[f] || pending.has(f)) return;
    pending.add(f);
    api.source(f)
      .then((s) => (cache[f] = s.lines))
      .catch((e) => {
        cache[f] = [`(source unavailable: ${e instanceof Error ? e.message : e})`];
      })
      .finally(() => pending.delete(f));
  });

  $effect(() => {
    void [src?.line, lines];
    tick().then(() => body?.querySelector(".cur")?.scrollIntoView({ block: "center" }));
  });
</script>

<div class="panel">
  <h2>Source {src ? `· ${src.file}:${src.line}` : ""}</h2>
  <div class="body mono" bind:this={body}>
    {#if !src}
      <div class="dim pad">no source for {lab.cur?.loc ?? ""}</div>
    {:else if !lines}
      <div class="dim pad">loading…</div>
    {:else}
      {#each lines as text, i}
        <div class="ln" class:cur={i + 1 === src.line}><span class="num dim">{i + 1}</span><span class="txt">{text}</span></div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .pad { padding: 8px; }
  .ln { display: flex; white-space: pre; }
  .num { min-width: 3.5em; text-align: right; padding-right: 1em; user-select: none; }
  .cur { background: var(--hl); }
</style>
