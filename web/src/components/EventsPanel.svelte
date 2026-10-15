<script lang="ts">
  import { lab } from "../lib/state.svelte";

  let { kind }: { kind: "events" | "abi" } = $props();

  const events = $derived((lab.timeline?.markers ?? []).filter((m) => m.kind === "syscall" || m.kind === "signal"));
</script>

<div class="list mono">
  {#if kind === "events"}
    {#each events as m}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="item clickable" class:cur={m.step === lab.step} onclick={() => lab.goto(m.step)}>
        <span class="step dim">#{m.step}</span><span class="k {m.kind}">{m.kind}</span><span>{m.label}</span>
      </div>
    {:else}
      <div class="dim pad">no syscalls or signals</div>
    {/each}
  {:else}
    {#each lab.findings as f}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="item clickable" class:cur={f.step === lab.step} onclick={() => lab.goto(f.step)}>
        <span class="step dim">#{f.step}</span><span class="k abi">{f.rule}</span><span class="dim">{f.loc}</span><span>{f.message}</span>
      </div>
    {:else}
      <div class="dim pad">no ABI findings</div>
    {/each}
  {/if}
</div>

<style>
  .pad { padding: 8px; }
  .item { display: flex; gap: 12px; padding: 1px 8px; white-space: pre; }
  .cur { background: var(--hl); }
  .step { min-width: 8ch; text-align: right; }
  .k { min-width: 10ch; font-weight: 700; }
  .syscall { color: var(--c3); }
  .signal { color: var(--red); }
  .abi { color: var(--orange); }
</style>
