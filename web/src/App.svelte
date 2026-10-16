<script lang="ts">
  import { onMount } from "svelte";
  import { lab } from "./lib/state.svelte";
  import TopBar from "./components/TopBar.svelte";
  import Timeline from "./components/Timeline.svelte";
  import SourcePanel from "./components/SourcePanel.svelte";
  import DisasmPanel from "./components/DisasmPanel.svelte";
  import RegistersPanel from "./components/RegistersPanel.svelte";
  import StackPanel from "./components/StackPanel.svelte";
  import MemoryPanel from "./components/MemoryPanel.svelte";
  import EventsPanel from "./components/EventsPanel.svelte";

  let tab = $state<"Memory" | "Events" | "ABI">("Memory");
  let locInput = $state<HTMLInputElement | null>(null);

  onMount(() => lab.init());

  function onkey(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable)) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    let handled = true;
    switch (e.key) {
      case "ArrowRight": case "l": lab.stepBy(1); break;
      case "ArrowLeft": case "h": lab.stepBy(-1); break;
      case "n": lab.nav("next-line"); break;
      case "N": lab.nav("prev-line"); break;
      case "s": lab.nav("over"); break;
      case "S": lab.nav("back-over"); break;
      case "o": lab.nav("out"); break;
      case "O": lab.nav("back-out"); break;
      case "g": locInput?.focus(); locInput?.select(); break;
      case "Home": lab.goto(0); break;
      case "End": lab.goto(lab.last); break;
      default: handled = false;
    }
    if (handled) e.preventDefault();
  }
</script>

<svelte:window onkeydown={onkey} />

<div class="app">
  <TopBar bind:locInput />
  <Timeline />
  <div class="main">
    <div class="col left">
      <SourcePanel />
      <DisasmPanel />
    </div>
    <RegistersPanel />
    <StackPanel />
  </div>
  <div class="bottom panel">
    <div class="tabs">
      {#each ["Memory", "Events", "ABI"] as const as t}
        <button class:active={tab === t} onclick={() => (tab = t)}>{t}{t === "ABI" && lab.findings.length ? ` (${lab.findings.length})` : ""}</button>
      {/each}
    </div>
    <div class="tabbody">
      {#if tab === "Memory"}<MemoryPanel />{:else if tab === "Events"}<EventsPanel kind="events" />{:else}<EventsPanel kind="abi" />{/if}
    </div>
  </div>
  <div class="footer dim">
    →/l next · ←/h prev · n/N next/prev line · s/S over/back-over · o/O out/back-out · g locator · Home/End first/last
  </div>
  {#if lab.error}
    <div class="toast" role="alert">
      <span>{lab.error}</span>
      <button onclick={() => (lab.error = null)} aria-label="Dismiss">×</button>
    </div>
  {/if}
</div>

<style>
  .app {
    height: 100%; display: grid; gap: 4px; padding: 4px;
    grid-template-rows: 40px 56px minmax(0, 1fr) 260px 20px;
    grid-template-columns: minmax(0, 1fr);
  }
  .main { display: grid; gap: 4px; grid-template-columns: minmax(0, 1.3fr) minmax(0, 1fr) minmax(0, 1.2fr); min-height: 0; }
  .col { display: grid; gap: 4px; grid-template-rows: minmax(0, 1fr) minmax(0, 1fr); min-height: 0; min-width: 0; }
  .tabs { display: flex; gap: 2px; padding: 3px 4px 0; border-bottom: 1px solid var(--border); }
  .tabs button { border-bottom: none; border-radius: 4px 4px 0 0; }
  .tabs button.active { background: var(--cur); border-color: var(--accent); }
  .tabbody { flex: 1; min-height: 0; overflow: auto; }
  .footer { font-size: 11px; padding: 0 4px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .toast {
    position: fixed; right: 12px; bottom: 28px; max-width: 480px; display: flex; gap: 10px; align-items: flex-start;
    background: var(--red); color: #fff; padding: 6px 10px; border-radius: 6px; box-shadow: 0 2px 8px #0006; z-index: 10;
  }
  .toast button { background: transparent; color: #fff; border-color: #fff8; padding: 0 6px; }
</style>
