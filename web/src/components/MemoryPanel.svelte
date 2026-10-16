<script lang="ts">
  import { api, type Mem } from "../lib/api";
  import { big, hex } from "../lib/format";
  import { lab } from "../lib/state.svelte";

  const LEN = 256;
  const ROW = 16;
  const WIDTHS = [1, 2, 4, 8] as const;

  type Cell = { addr: string; size: number; dbl: string; bytes: { i: number; text: string; cls: string; title: string }[] };

  let mem = $state.raw<Mem | null>(null);
  let note = $state("");
  /** `lab.memAddr` after a short pause, so half-typed expressions are not evaluated. */
  let expr = $state(lab.memAddr.trim());

  $effect(() => {
    const v = lab.memAddr.trim();
    const t = setTimeout(() => (expr = v), 200);
    return () => clearTimeout(t);
  });

  // Resolve the address (empty follows %rsp) and read 256 bytes from the 16-byte boundary at or below it.
  $effect(() => {
    const c = lab.cur;
    void lab.status.generation;
    const text = expr;
    if (!c) return;
    const ctl = new AbortController();
    void (async () => {
      let base: bigint;
      if (text === "") {
        const rsp = c.regs.find((r) => r.name === "rsp");
        if (!rsp) return;
        base = big(rsp.value);
      } else if (/^0x[0-9a-f]+$/i.test(text)) {
        base = big(text);
      } else {
        try {
          base = big((await api.eval(text, c.step)).value);
        } catch (e) {
          if (ctl.signal.aborted) return;
          mem = null;
          note = `cannot resolve "${text}": ${e instanceof Error ? e.message : e}`;
          return;
        }
        if (ctl.signal.aborted) return;
      }
      try {
        const m = await api.mem(hex(BigInt.asUintN(64, base) & ~15n), LEN, c.step, ctl.signal);
        mem = m;
        note = "";
      } catch (e) {
        lab.fail(e);
      }
    })();
    return () => ctl.abort();
  });

  /** Offsets (from `start`) of the bytes in `[addr, addr + size)` that fall inside the window, added to `into`. */
  function mark(into: Set<number>, addr: bigint, size: number, start: bigint) {
    const off = addr - start;
    if (off >= BigInt(LEN) || off + BigInt(size) <= 0n) return;
    const lo = Math.max(0, Number(off));
    const hi = Math.min(LEN, Number(off) + size);
    for (let i = lo; i < hi; i++) into.add(i);
  }

  const start = $derived(mem ? big(mem.addr) : 0n);

  const grid = $derived.by<Cell[][]>(() => {
    const m = mem;
    if (!m) return [];
    const width = lab.prefs.memWidth;
    const hot = new Set<number>();
    for (const w of lab.cur?.prev_writes ?? []) mark(hot, big(w.raw_addr), w.size, start);
    const next = new Set<number>();
    for (const w of lab.cur?.effects?.mem_writes ?? []) mark(next, big(w.raw_addr), w.size, start);
    const chosen = new Set<number>();
    const s = lab.selection;
    if (s?.kind === "mem") mark(chosen, big(s.addr), s.size, start);

    const rows: Cell[][] = [];
    for (let r = 0; r < LEN / ROW; r++) {
      const row: Cell[] = [];
      for (let c = 0; c < ROW / width; c++) {
        const lo = r * ROW + c * width;
        let latest = lo;
        const bytes: Cell["bytes"] = [];
        // Highest address first, so a value reads the way it is written.
        for (let k = width - 1; k >= 0; k--) {
          const i = lo + k;
          const b = m.bytes[i] ?? null;
          const w = m.writers[i] ?? null;
          if (w !== null && w >= (m.writers[latest] ?? -1)) latest = i;
          const cls = ["b"];
          if (b === null) cls.push("nul");
          if (hot.has(i)) cls.push("hot");
          // The left edge of a run is its highest address; runs end at cell boundaries.
          const left = k === width - 1 || !chosen.has(i + 1);
          const right = k === 0 || !chosen.has(i - 1);
          if (chosen.has(i)) cls.push("sel", ...(left ? ["sl"] : []), ...(right ? ["sr"] : []));
          const nextLeft = k === width - 1 || !next.has(i + 1);
          const nextRight = k === 0 || !next.has(i - 1);
          if (next.has(i)) cls.push("nx", ...(nextLeft ? ["nl"] : []), ...(nextRight ? ["nr"] : []));
          const where = hex(start + BigInt(i));
          bytes.push({
            i,
            text: b === null ? "··" : b.toString(16).padStart(2, "0"),
            cls: cls.join(" "),
            title: `${where}: ${b === null ? "unmapped" : w === null ? "initial contents" : `written at step ${w}`}`,
          });
        }
        row.push({ addr: hex(start + BigInt(lo)), size: width, dbl: hex(start + BigInt(latest)), bytes });
      }
      rows.push(row);
    }
    return rows;
  });

  const ascii = (r: number) =>
    Array.from({ length: ROW }, (_, c) => {
      const b = mem?.bytes[r * ROW + c] ?? null;
      return b === null ? "·" : b >= 32 && b < 127 ? String.fromCharCode(b) : ".";
    }).join("");

  function setWidth(v: string) {
    lab.prefs.memWidth = Number(v) as (typeof WIDTHS)[number];
    lab.savePrefs();
  }
</script>

<div class="mem mono">
  <div class="bar">
    <label class="dim">address
      <input bind:value={lab.memAddr} placeholder="following %rsp" spellcheck="false" autocomplete="off" />
    </label>
    {#if lab.memAddr}<button onclick={() => (lab.memAddr = "")}>follow rsp</button>{/if}
    <label class="dim">width
      <select value={lab.prefs.memWidth} onchange={(e) => setWidth(e.currentTarget.value)}>
        {#each WIDTHS as w (w)}<option value={w}>{w} byte{w === 1 ? "" : "s"}</option>{/each}
      </select>
    </label>
    {#if note}<span class="warn">{note}</span>{/if}
    <span class="legend dim">filled: written by the previous instruction · outlined: written by the next one</span>
  </div>
  {#if mem}
    <div class="grid">
      {#each grid as row, r (r)}
        <div class="row">
          <span class="a dim">{hex(start + BigInt(r * ROW))}</span>
          <span class="cells">
            {#each row as cell (cell.addr)}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <span
                class="cell clickable"
                onclick={() => lab.select({ kind: "mem", addr: cell.addr, size: cell.size })}
                ondblclick={() => lab.nav("write-prev", { addr: cell.dbl })}
              >
                {#each cell.bytes as b (b.i)}<span class={b.cls} title={b.title}>{b.text}</span>{/each}
              </span>
            {/each}
          </span>
          <span class="asc dim">{ascii(r)}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .mem { padding: 4px 8px; }
  .bar { display: flex; flex-wrap: wrap; gap: 4px 12px; align-items: center; margin-bottom: 4px; font-family: var(--font); }
  .bar input { width: 260px; font-family: var(--mono); }
  .warn { color: var(--red); }
  .legend { margin-left: auto; font-size: 11px; }
  .row { display: flex; gap: 14px; white-space: pre; }
  .a { min-width: 14ch; }
  .cells { display: inline-flex; gap: 6px; }
  .cell { border-radius: 2px; }
  .b { border-top: 1px solid transparent; border-bottom: 1px solid transparent; padding: 0 1px; box-shadow: var(--s-t, 0 0 #0000), var(--s-b, 0 0 #0000), var(--s-l, 0 0 #0000), var(--s-r, 0 0 #0000); }
  .nul { color: var(--dim); }
  .hot { background: var(--changed); font-weight: 700; }
  .nx { border-top: 1px dashed var(--orange); border-bottom: 1px dashed var(--orange); }
  .nx.nl { border-left: 1px dashed var(--orange); padding-left: 0; }
  .nx.nr { border-right: 1px dashed var(--orange); padding-right: 0; }
  .sel { --s-t: inset 0 1px var(--accent); --s-b: inset 0 -1px var(--accent); }
  .sel.sl { --s-l: inset 1px 0 var(--accent); }
  .sel.sr { --s-r: inset -1px 0 var(--accent); }
</style>
