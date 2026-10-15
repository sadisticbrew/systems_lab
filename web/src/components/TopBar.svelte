<script lang="ts">
  import { lab } from "../lib/state.svelte";

  let { locInput = $bindable(null) }: { locInput?: HTMLInputElement | null } = $props();
  let text = $state("");

  const end = $derived.by(() => {
    const e = lab.info?.end;
    if (!e) return "";
    if (e.reason === "exit") return `exit ${e.code ?? 0}`;
    if (e.reason === "signal") return `signal ${e.signal ?? "?"}`;
    return "max-steps";
  });

  function submit(e: KeyboardEvent) {
    if (e.key === "Enter" && text.trim()) {
      lab.locate(text.trim());
    } else if (e.key === "Escape") {
      (e.target as HTMLInputElement).blur();
    }
  }
</script>

<div class="top panel">
  <strong class="mono">{lab.info?.program ?? "SystemsLab"}</strong>
  <span class="mono step">{lab.step} / {lab.last}</span>
  <input
    class="mono"
    bind:this={locInput}
    bind:value={text}
    onkeydown={submit}
    placeholder="locator: fact+0xc@3, step:42, 0x401136 (g)"
    spellcheck="false"
  />
  <span class="dim">from {lab.info?.from ?? ""}</span>
  <span class="dim">ended: {end}</span>
  {#if lab.info && lab.info.unattributed_bytes > 0}
    <span class="badge" title="memory changed by instructions the recorder could not attribute">
      {lab.info.unattributed_bytes} unattributed bytes
    </span>
  {/if}
</div>

<style>
  .top { flex-direction: row; align-items: center; gap: 12px; padding: 0 10px; }
  .step { min-width: 90px; }
  input { width: 280px; }
  .badge { background: var(--red); color: #fff; padding: 1px 8px; border-radius: 10px; font-size: 12px; }
</style>
