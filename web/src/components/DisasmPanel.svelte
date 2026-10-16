<script lang="ts">
  import { tick, untrack } from "svelte";
  import { api, type Disasm } from "../lib/api";
  import { lab } from "../lib/state.svelte";

  let dis = $state<Disasm | null>(null);
  let body: HTMLDivElement;
  let seq = 0;

  const rip = $derived(lab.cur?.regs.find((r) => r.name === "rip")?.value ?? null);
  const ripN = $derived(rip ? BigInt(rip) : null);
  const base = (loc: string) => loc.split("+")[0];
  const funcKey = $derived(lab.cur ? base(lab.cur.loc) : null);

  $effect(() => {
    const key = funcKey;
    const r = rip;
    const step = lab.step;
    if (!key || !r) return;
    untrack(() => {
      // Bump first so a response still in flight for another function cannot replace a listing that already fits.
      const id = ++seq;
      const have = dis && dis.func === key && dis.lines.some((l) => BigInt(l.addr) === BigInt(r));
      if (have) return;
      api.disasm(r, step)
        .then((d) => { if (id === seq) dis = { ...d, func: key }; })
        .catch((e) => lab.fail(e));
    });
  });

  $effect(() => {
    void [ripN, dis];
    tick().then(() => body?.querySelector(".cur")?.scrollIntoView({ block: "center" }));
  });
</script>

<div class="panel">
  <h2>Disassembly {dis ? `· ${dis.func}` : ""}</h2>
  <div class="body mono" bind:this={body}>
    {#if dis}
      {#each dis.lines as l (l.addr)}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="ln clickable" class:cur={ripN !== null && BigInt(l.addr) === ripN} onclick={() => lab.nav("at-next", { addr: l.addr })} title={l.addr}>
          <span class="loc dim">{l.loc}</span>
          <span class="bytes dim">{l.bytes}</span>
          <span class="asm">{l.asm}</span>
        </div>
      {/each}
    {:else}
      <div class="dim pad">no disassembly</div>
    {/if}
  </div>
</div>

<style>
  .pad { padding: 8px; }
  .ln { display: flex; gap: 12px; white-space: pre; padding: 0 6px; }
  .loc { min-width: 12ch; flex: none; }
  .bytes { min-width: 16ch; flex: none; }
  .cur, .cur:hover { background: var(--hl); }
</style>
