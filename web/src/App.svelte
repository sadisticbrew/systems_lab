<script lang="ts">
  import { onMount } from "svelte";
  import { lab, PREF_DEFAULTS, type BottomTab } from "./lib/state.svelte";
  import TopBar from "./components/TopBar.svelte";
  import Banner from "./components/Banner.svelte";
  import Timeline from "./components/Timeline.svelte";
  import Splitter from "./components/Splitter.svelte";
  import HelpOverlay from "./components/HelpOverlay.svelte";
  import SourcePanel from "./components/SourcePanel.svelte";
  import DisasmPanel from "./components/DisasmPanel.svelte";
  import EffectsPanel from "./components/EffectsPanel.svelte";
  import RegistersPanel from "./components/RegistersPanel.svelte";
  import Inspector from "./components/Inspector.svelte";
  import WatchPanel from "./components/WatchPanel.svelte";
  import StackPanel from "./components/StackPanel.svelte";
  import MemoryPanel from "./components/MemoryPanel.svelte";
  import EventsPanel from "./components/EventsPanel.svelte";
  import OutputPanel from "./components/OutputPanel.svelte";
  import AbiPanel from "./components/AbiPanel.svelte";
  import NotesPanel from "./components/NotesPanel.svelte";
  import GuidePanel from "./components/GuidePanel.svelte";

  /** Below this window width the three columns become tabs. */
  const NARROW_BELOW = 1100;
  const MIN_COL = 0.15;
  const SPLITTER = 6;
  const NARROW_TABS = [["code", "Code"], ["regs", "Registers"], ["stack", "Stack"]] as const;

  let innerWidth = $state(window.innerWidth);
  let innerHeight = $state(window.innerHeight);
  let locInput = $state<HTMLInputElement | null>(null);
  let mainEl = $state<HTMLDivElement>();
  let srcEl = $state<HTMLDivElement>();
  let disEl = $state<HTMLDivElement>();

  const narrow = $derived(innerWidth < NARROW_BELOW);
  const collapsed = $derived(lab.prefs.bottomCollapsed);
  const bottomPx = $derived(Math.min(Math.max(lab.prefs.bottom, 120), innerHeight * 0.7));
  const tabs = $derived<BottomTab[]>(["Memory", "Events", "Output", "ABI", "Notes", ...(lab.info?.has_guide ? (["Guide"] as const) : [])]);
  const tab = $derived(tabs.includes(lab.bottomTab) ? lab.bottomTab : "Memory");
  // Thousandths keep the fr factors above 1 in total, so the columns always fill the row.
  const columns = $derived(lab.prefs.cols.flatMap((c, i) => (i === 0 ? [`minmax(0, ${c * 1000}fr)`] : [`${SPLITTER}px`, `minmax(0, ${c * 1000}fr)`])).join(" "));

  onMount(() => lab.init());

  const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), hi);

  // --- splitters: sizes come from the values captured when the drag starts, so clamping never drifts ---
  let colStart = { cols: [0, 0, 0], width: 1 };
  let bottomStart = 0;
  let leftStart = { top: 0, both: 1 };

  function startCols() {
    colStart = { cols: [...lab.prefs.cols], width: Math.max(1, (mainEl?.clientWidth ?? 1) - 2 * SPLITTER) };
  }

  /** Moves the boundary after column `i`, keeping the neighbours at least `MIN_COL` of the total. */
  function dragCols(i: 0 | 1, dx: number) {
    const c = colStart.cols;
    const total = c[0] + c[1] + c[2];
    const d = clamp((dx / colStart.width) * total, MIN_COL * total - c[i], c[i + 1] - MIN_COL * total);
    const next = [...c];
    next[i] += d;
    next[i + 1] -= d;
    lab.prefs.cols = next as [number, number, number];
  }

  function startLeft() {
    const top = srcEl?.offsetHeight ?? 0;
    leftStart = { top, both: Math.max(1, top + (disEl?.offsetHeight ?? 0)) };
  }

  function toggleBottom() {
    lab.prefs.bottomCollapsed = !lab.prefs.bottomCollapsed;
    lab.savePrefs();
  }

  function pickTab(t: BottomTab) {
    lab.bottomTab = t;
    if (lab.prefs.bottomCollapsed) toggleBottom();
  }

  function pickNarrow(t: "code" | "regs" | "stack") {
    lab.prefs.narrowTab = t;
    lab.savePrefs();
  }

  // --- keyboard ---
  const TYPING = "textarea, select, [contenteditable], input:not([type=checkbox], [type=radio], [type=button], [type=range])";

  function changeOfSelection(dir: "prev" | "next") {
    const sel = lab.selection;
    if (!sel) {
      lab.notify("Select a register, memory byte or stack slot first.");
    } else if (sel.kind === "reg") {
      lab.nav(dir === "prev" ? "reg-prev" : "reg-next", { reg: sel.name });
    } else {
      lab.nav(dir === "prev" ? "write-prev" : "write-next", { addr: sel.addr });
    }
  }

  function onkey(e: KeyboardEvent) {
    const typing = e.target instanceof Element && e.target.matches(TYPING);
    if (e.key === "Escape") {
      if (lab.helpOpen) lab.helpOpen = false;
      else if (typing) (e.target as HTMLElement).blur();
      else lab.select(null);
      return;
    }
    if (typing || e.ctrlKey || e.metaKey || e.altKey) return;
    if (lab.helpOpen && e.key !== "?") return;
    let handled = true;
    switch (e.key) {
      case "ArrowRight": case "l": lab.stepBy(1); break;
      case "ArrowLeft": case "h": lab.stepBy(-1); break;
      case "s": lab.nav("over"); break;
      case "S": lab.nav("back-over"); break;
      case "n": lab.nav("next-line"); break;
      case "N": lab.nav("prev-line"); break;
      case "o": lab.nav("out"); break;
      case "O": lab.nav("back-out"); break;
      case "[": changeOfSelection("prev"); break;
      case "]": changeOfSelection("next"); break;
      case "Backspace": if (e.shiftKey) lab.forward(); else lab.back(); break;
      case "g": locInput?.focus(); locInput?.select(); break;
      case "m": lab.addBookmark(); break;
      case "p": lab.prefs.predict = !lab.prefs.predict; lab.savePrefs(); break;
      case "?": lab.helpOpen = !lab.helpOpen; break;
      case "Home": lab.goto(0); break;
      case "End": lab.goto(lab.last); break;
      default: handled = false;
    }
    if (handled) e.preventDefault();
  }
</script>

<svelte:window onkeydown={onkey} bind:innerWidth bind:innerHeight />

{#snippet leftCol()}
  <div class="col left">
    <div class="slot" bind:this={srcEl} style:flex="{lab.prefs.leftSplit * 1000} 1 0"><SourcePanel /></div>
    <Splitter
      orientation="horizontal"
      label="Resize source and disassembly"
      onstart={startLeft}
      ondrag={(dy) => (lab.prefs.leftSplit = clamp((leftStart.top + dy) / leftStart.both, 0.15, 0.85))}
      onend={() => lab.savePrefs()}
      onreset={() => { lab.prefs.leftSplit = PREF_DEFAULTS.leftSplit; lab.savePrefs(); }}
    />
    <div class="slot" bind:this={disEl} style:flex="{(1 - lab.prefs.leftSplit) * 1000} 1 0"><DisasmPanel /></div>
    <div class="slot effects"><EffectsPanel /></div>
  </div>
{/snippet}

{#snippet midCol()}
  <div class="col mid">
    <div class="slot regs"><RegistersPanel /></div>
    {#if lab.selection}<div class="slot inspector"><Inspector /></div>{/if}
    <div class="slot watch"><WatchPanel /></div>
  </div>
{/snippet}

{#snippet rightCol()}
  <div class="col right"><div class="slot grow"><StackPanel /></div></div>
{/snippet}

<div class="app">
  <TopBar bind:locInput />
  <Banner />
  <div class="tlrow"><Timeline /></div>
  <div class="work">
    {#if narrow}
      <div class="strip" role="tablist">
        {#each NARROW_TABS as [key, label]}
          <button role="tab" aria-selected={lab.prefs.narrowTab === key} class:active={lab.prefs.narrowTab === key} onclick={() => pickNarrow(key)}>{label}</button>
        {/each}
      </div>
      <div class="main narrow">
        {#if lab.prefs.narrowTab === "code"}{@render leftCol()}{:else if lab.prefs.narrowTab === "regs"}{@render midCol()}{:else}{@render rightCol()}{/if}
      </div>
    {:else}
      <div class="main wide" bind:this={mainEl} style:grid-template-columns={columns}>
        {@render leftCol()}
        <Splitter
          orientation="vertical"
          label="Resize code and registers"
          onstart={startCols}
          ondrag={(dx) => dragCols(0, dx)}
          onend={() => lab.savePrefs()}
          onreset={() => { lab.prefs.cols = [...PREF_DEFAULTS.cols]; lab.savePrefs(); }}
        />
        {@render midCol()}
        <Splitter
          orientation="vertical"
          label="Resize registers and stack"
          onstart={startCols}
          ondrag={(dx) => dragCols(1, dx)}
          onend={() => lab.savePrefs()}
          onreset={() => { lab.prefs.cols = [...PREF_DEFAULTS.cols]; lab.savePrefs(); }}
        />
        {@render rightCol()}
      </div>
    {/if}
    {#if collapsed}
      <div class="spacer"></div>
    {:else}
      <Splitter
        orientation="horizontal"
        label="Resize the bottom panel"
        onstart={() => (bottomStart = bottomPx)}
        ondrag={(dy) => (lab.prefs.bottom = Math.round(clamp(bottomStart - dy, 120, innerHeight * 0.7)))}
        onend={() => lab.savePrefs()}
        onreset={() => { lab.prefs.bottom = PREF_DEFAULTS.bottom; lab.savePrefs(); }}
      />
    {/if}
    <div class="bottom panel" style:height={collapsed ? "28px" : `${bottomPx}px`}>
      <div class="tabs" role="tablist">
        {#each tabs as t}
          <button role="tab" aria-selected={tab === t} class:active={tab === t && !collapsed} onclick={() => pickTab(t)}>
            {t}{t === "ABI" && lab.findings.length ? ` (${lab.findings.length})` : ""}
          </button>
        {/each}
        <button class="chevron" onclick={toggleBottom} title={collapsed ? "Show the bottom panel" : "Hide the bottom panel"} aria-label={collapsed ? "Show the bottom panel" : "Hide the bottom panel"}>
          {collapsed ? "▴" : "▾"}
        </button>
      </div>
      {#if !collapsed}
        <div class="tabbody">
          {#if tab === "Memory"}<MemoryPanel />
          {:else if tab === "Events"}<EventsPanel />
          {:else if tab === "Output"}<OutputPanel />
          {:else if tab === "ABI"}<AbiPanel />
          {:else if tab === "Notes"}<NotesPanel />
          {:else}<GuidePanel />{/if}
        </div>
      {/if}
    </div>
  </div>
  <div class="footer" role="status">
    {#if lab.notice}<span class="notice">{lab.notice}</span>{:else}<span class="dim">? for help</span>{/if}
  </div>
  {#if lab.error}
    <div class="toast" role="alert">
      <span>{lab.error}</span>
      <button onclick={() => (lab.error = null)} aria-label="Dismiss">×</button>
    </div>
  {/if}
  {#if lab.helpOpen}<HelpOverlay />{/if}
</div>

<style>
  .app { height: 100%; display: flex; flex-direction: column; gap: 4px; padding: 4px; }
  .tlrow { height: 74px; flex: none; display: flex; flex-direction: column; }
  .tlrow > :global(.panel) { flex: 1; }
  .work { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .main { flex: 1; min-height: 0; }
  .main.wide { display: grid; }
  .main.narrow { display: flex; flex-direction: column; }
  .strip { display: flex; gap: 2px; margin-bottom: 4px; }
  .strip button, .tabs button { border-bottom: none; border-radius: 4px 4px 0 0; }
  .strip button.active, .tabs button.active { background: var(--cur); border-color: var(--accent); }
  .col { display: flex; flex-direction: column; min-height: 0; min-width: 0; }
  .col.mid { gap: 4px; }
  .narrow .col { flex: 1; }
  .slot { display: flex; flex-direction: column; min-height: 0; min-width: 0; }
  .slot > :global(.panel) { flex: 1 1 auto; }
  .slot.grow { flex: 1 1 0; }
  .effects { flex: 0 1 auto; max-height: 45%; margin-top: 4px; }
  .regs { flex: 1 1 0; min-height: 80px; }
  .inspector { flex: 0 1 auto; max-height: 45%; }
  .watch { flex: 0 1 auto; max-height: 35%; }
  .spacer { height: 6px; flex: none; }
  .bottom { flex: none; }
  .tabs { display: flex; align-items: flex-end; gap: 2px; padding: 3px 4px 0; height: 28px; flex: none; }
  .tabs:not(:last-child) { border-bottom: 1px solid var(--border); }
  .chevron { margin-left: auto; margin-bottom: 2px; border-radius: 4px; padding: 0 8px; }
  .tabbody { flex: 1; min-height: 0; overflow: auto; }
  .footer { height: 20px; flex: none; font-size: 11px; padding: 0 4px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .notice { color: var(--accent); }
  .toast {
    position: fixed; right: 12px; bottom: 28px; max-width: 480px; display: flex; gap: 10px; align-items: flex-start;
    background: var(--red); color: #fff; padding: 6px 10px; border-radius: 6px; box-shadow: 0 2px 8px #0006; z-index: 10;
  }
  .toast button { background: transparent; color: #fff; border-color: #fff8; padding: 0 6px; }
</style>
