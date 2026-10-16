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

  const atStart = $derived(lab.step <= 0);
  const atEnd = $derived(lab.step >= lab.last);

  const groups = [
    {
      label: "Step",
      back: ["Step backwards (←)", () => lab.stepBy(-1)],
      fwd: ["Step forwards (→)", () => lab.stepBy(1)],
    },
    {
      label: "Over",
      back: ["Step over backwards (S)", () => lab.nav("back-over")],
      fwd: ["Step over forwards (s)", () => lab.nav("over")],
    },
    {
      label: "Line",
      back: ["Previous source line (N)", () => lab.nav("prev-line")],
      fwd: ["Next source line (n)", () => lab.nav("next-line")],
    },
    {
      label: "Out",
      back: ["Step out backwards (O)", () => lab.nav("back-out")],
      fwd: ["Step out forwards (o)", () => lab.nav("out")],
    },
  ] as const;

  async function submit(e: KeyboardEvent) {
    if (e.key !== "Enter" || !text.trim()) return;
    if (await lab.locate(text.trim())) {
      text = "";
      locInput?.blur(); // otherwise Backspace would edit the box instead of going back
    }
  }

  async function copy() {
    const loc = lab.cur?.locator;
    if (!loc) return;
    try {
      await navigator.clipboard.writeText(loc);
      lab.notify(`Copied ${loc}`);
    } catch {
      lab.fail("Could not copy to the clipboard.");
    }
  }
</script>

<div class="top panel">
  <strong class="mono name" title={lab.info?.program}>{lab.info?.program.split("/").pop() ?? "SystemsLab"}</strong>
  <span class="hist">
    <button disabled={!lab.canBack} onclick={() => lab.back()} title="Back to where you were (Backspace)" aria-label="Back">‹<span class="word"> Back</span></button>
    <button disabled={!lab.canForward} onclick={() => lab.forward()} title="Forward again (Shift+Backspace)" aria-label="Forward"><span class="word">Forward </span>›</button>
  </span>
  {#each groups as g}
    <span class="grp">
      <button disabled={atStart} title={g.back[0]} aria-label={g.back[0]} onclick={g.back[1]}>◀</button>
      <span class="dim lbl">{g.label}</span>
      <button disabled={atEnd} title={g.fwd[0]} aria-label={g.fwd[0]} onclick={g.fwd[1]}>▶</button>
    </span>
  {/each}
  <span class="mono step">{lab.step} / {lab.last}</span>
  <button class="mono loc" onclick={copy} title="Click to copy this locator">{lab.cur?.locator ?? ""}</button>
  <input
    class="mono"
    list="fn-names"
    bind:this={locInput}
    bind:value={text}
    onkeydown={submit}
    placeholder="go to: fact+0xc@3, fact.c:4, step:42 (g)"
    spellcheck="false"
  />
  <datalist id="fn-names">
    {#each lab.info?.functions ?? [] as f (f.name)}<option value={f.name}></option>{/each}
  </datalist>
  <span class="dim from" title={lab.info?.argv.join(" ")}>from {lab.info?.from ?? ""}</span>
  <span class="dim ended">ended: {end}</span>
  {#if lab.info && lab.info.unattributed_bytes > 0}
    <span class="badge" title="memory changed by instructions the recorder could not attribute">
      {lab.info.unattributed_bytes} unattributed bytes
    </span>
  {/if}
  <button class="help" onclick={() => (lab.helpOpen = !lab.helpOpen)} title="Help (?)" aria-label="Help">?</button>
</div>

<style>
  .top { flex-direction: row; align-items: center; gap: 10px; padding: 0 10px; height: 40px; flex: none; }
  .hist, .grp, .step, .loc, .badge, .help, button { flex: none; }
  .name { flex: 0 1 auto; min-width: 0; max-width: 16ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hist, .grp { display: inline-flex; align-items: center; gap: 2px; white-space: nowrap; }
  .hist { margin-right: 4px; }
  .grp button { padding: 0 6px; font-size: 11px; line-height: 20px; }
  .lbl { min-width: 3.4ch; text-align: center; font-size: 11px; }
  .step { min-width: 11ch; text-align: right; }
  .loc { border-color: transparent; background: transparent; padding: 0 4px; white-space: nowrap; }
  input { flex: 1 1 120px; min-width: 100px; max-width: 280px; }
  .from, .ended { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; flex: 0 1 auto; min-width: 0; }
  .badge { background: var(--red); color: #fff; padding: 1px 8px; border-radius: 10px; font-size: 12px; white-space: nowrap; }
  .help { margin-left: auto; width: 24px; padding: 0; border-radius: 12px; }
  @media (max-width: 1350px) { .from { display: none; } }
  @media (max-width: 1250px) { .word { display: none; } }
</style>
