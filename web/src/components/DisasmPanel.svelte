<script lang="ts">
  import { tick, untrack } from "svelte";
  import { api, type Disasm } from "../lib/api";
  import { lab } from "../lib/state.svelte";
  import { asmTokens, type AsmToken } from "../lib/format";
  import { revealRow } from "../lib/reveal";

  type Row =
    | { kind: "head"; key: string; where: string; text: string }
    | { kind: "line"; key: string; addr: string; loc: string; bytes: string; tokens: AsmToken[]; hits: number };

  let dis = $state.raw<Disasm | null>(null);
  let body = $state<HTMLDivElement>();
  let seq = 0;
  let fetchedGeneration = lab.status.generation;

  const rip = $derived(lab.cur?.regs.find((r) => r.name === "rip")?.value ?? null);
  const ripN = $derived(rip ? BigInt(rip) : null);
  const base = (loc: string) => loc.split("+")[0];
  const funcKey = $derived(lab.cur ? base(lab.cur.loc) : null);

  $effect(() => {
    const key = funcKey;
    const r = rip;
    const step = lab.step;
    const generation = lab.status.generation;
    if (!key || !r) return;
    untrack(() => {
      // Bump first so a response still in flight for another function cannot replace a listing that already fits.
      const id = ++seq;
      const stale = generation !== fetchedGeneration;
      const have = !stale && dis && dis.func === key && dis.lines.some((l) => BigInt(l.addr) === BigInt(r));
      if (have) return;
      api.disasm(r, step)
        .then((d) => {
          if (id !== seq) return;
          fetchedGeneration = generation;
          dis = { ...d, func: key };
        })
        .catch((e) => lab.fail(e));
    });
  });

  // The source text for the header rows comes from the shared cache.
  $effect(() => {
    for (const f of new Set(dis?.lines.map((l) => l.source_file))) if (f) lab.ensureSource(f);
  });

  /** The listing with a dim `file:line  text` row wherever the source line changes and its text is already cached. */
  const rows = $derived.by(() => {
    const out: Row[] = [];
    let file: string | null = null;
    let line: number | null = null;
    for (const l of dis?.lines ?? []) {
      if (l.source_line !== null && l.source_file !== null && (l.source_file !== file || l.source_line !== line)) {
        const text = lab.sources[l.source_file]?.[l.source_line - 1];
        if (text !== undefined) out.push({ kind: "head", key: `h${l.addr}`, where: `${l.source_file}:${l.source_line}`, text: text.trim() });
      }
      file = l.source_file;
      line = l.source_line;
      out.push({ kind: "line", key: l.addr, addr: l.addr, loc: l.loc, bytes: l.bytes, tokens: asmTokens(l.asm), hits: l.hits });
    }
    return out;
  });

  // Column widths follow the longest entry so the columns line up.
  const locW = $derived(Math.min(32, dis?.lines.reduce((w, l) => Math.max(w, l.loc.length), 8) ?? 8));
  const bytesW = $derived(dis?.lines.reduce((w, l) => Math.max(w, l.bytes.length), 8) ?? 8);

  $effect(() => {
    void [ripN, rows];
    tick().then(() => revealRow(body, body?.querySelector(".cur")));
  });
</script>

<div class="panel">
  <h2>Disassembly {dis ? `· ${dis.func}` : ""}</h2>
  <div class="body mono" bind:this={body} style:--loc-w="{locW}ch" style:--bytes-w="{bytesW}ch">
    {#if dis}
      {#each rows as row (row.key)}
        {#if row.kind === "head"}
          <div class="head dim"><span class="where">{row.where}</span><span class="src">{row.text}</span></div>
        {:else}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div class="ln clickable" class:cur={ripN !== null && BigInt(row.addr) === ripN} onclick={() => lab.nav("at-next", { addr: row.addr })} title="{row.addr}: jump to the next time this instruction runs">
            <span class="loc dim">{row.loc}</span>
            <span class="bytes dim">{row.bytes}</span>
            <span class="asm">{#each row.tokens as t}<span class={t.cls}>{t.text}</span>{/each}</span>
            <span class="hits dim" title={row.hits > 0 ? `runs ${row.hits} time${row.hits === 1 ? "" : "s"} in this recording` : ""}>{row.hits > 0 ? `×${row.hits}` : ""}</span>
          </div>
        {/if}
      {/each}
    {:else}
      <div class="dim pad">no disassembly</div>
    {/if}
  </div>
</div>

<style>
  .pad { padding: 8px; }
  .ln { display: flex; gap: 12px; white-space: pre; padding: 0 6px; }
  .head { display: flex; gap: 12px; white-space: pre; padding: 3px 6px 0; font-size: 12px; }
  .where { flex: none; }
  .src { overflow: hidden; text-overflow: ellipsis; }
  .loc { min-width: var(--loc-w); flex: none; }
  .bytes { min-width: var(--bytes-w); flex: none; }
  .asm { flex: 1 1 auto; }
  .hits { margin-left: auto; flex: none; padding-left: 8px; }
  .reg { color: var(--c3); }
  .imm { color: var(--orange); }
  .sym { color: var(--c2); }
  .cur, .cur:hover { background: var(--hl); }
</style>
