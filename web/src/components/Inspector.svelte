<script lang="ts">
  import { api, type Inspect, type NavOp, type NavResult } from "../lib/api";
  import { FLAGS, flagChanged } from "../lib/flags";
  import { big, binaryGroups, hex, signed, SUBREGS, unsigned } from "../lib/format";
  import { lab } from "../lib/state.svelte";

  type Probes = { sig: string; prev: NavResult; next: NavResult };
  type Fetched = { sig: string; data: Inspect };

  const INT_ROWS = [
    [8, "u8", "i8"],
    [16, "u16", "i16"],
    [32, "u32", "i32"],
    [64, "u64", "i64"],
  ] as const;
  const BYTE_GROUPS = [0, 1] as const;

  const sel = $derived(lab.selection);
  const sig = $derived(sel ? (sel.kind === "reg" ? `reg:${sel.name}` : `mem:${sel.addr}`) : "");

  // Where the selection changes next and last, measured from the displayed step. The previous answer stays up while a new one loads.
  let probes = $state.raw<Probes | null>(null);
  $effect(() => {
    const s = lab.selection;
    void lab.step;
    void lab.status.generation;
    if (!s) return;
    const key = sig;
    const isReg = s.kind === "reg";
    const extra = isReg ? { reg: s.name } : { addr: s.addr };
    const prevOp: NavOp = isReg ? "reg-prev" : "write-prev";
    const nextOp: NavOp = isReg ? "reg-next" : "write-next";
    let live = true;
    void Promise.all([lab.probe(prevOp, extra), lab.probe(nextOp, extra)]).then(([prev, next]) => {
      if (live) probes = { sig: key, prev, next };
    });
    return () => {
      live = false;
    };
  });

  let fetched = $state.raw<Fetched | null>(null);
  $effect(() => {
    const s = lab.selection;
    void lab.status.generation;
    if (s?.kind !== "mem") return;
    const key = sig;
    const ctl = new AbortController();
    api.inspect(s.addr, lab.step, ctl.signal).then(
      (data) => (fetched = { sig: key, data }),
      (e) => lab.fail(e),
    );
    return () => ctl.abort();
  });

  const probe = $derived(probes && probes.sig === sig ? probes : null);
  const mem = $derived(fetched && fetched.sig === sig ? fetched.data : null);
  const byType = $derived(new Map((mem?.values ?? []).map((v) => [v.type, v.value])));

  const reg = $derived(sel?.kind === "reg" ? (lab.cur?.regs.find((r) => r.name === sel.name) ?? null) : null);
  const val = $derived(reg ? big(reg.value) : 0n);
  const old = $derived(reg?.prev != null ? big(reg.prev) : null);
  const parts = $derived(reg ? (SUBREGS[reg.name] ?? [{ name: reg.name, bits: 64, shift: 0 }]) : []);
  const isGpr = $derived(reg !== null && reg.name in SUBREGS);
  const bitsNow = $derived(binaryGroups(val));
  const bitsOld = $derived(old === null ? null : binaryGroups(old));
  // The rendered form only earns a place in the header when it says more than the number does.
  const renderedNote = $derived(reg && reg.rendered !== unsigned(val, 64).toString() && reg.rendered !== signed(val, 64).toString() ? reg.rendered : "");

  const piece = (v: bigint, p: { bits: number; shift: number }) => unsigned(v >> BigInt(p.shift), p.bits);
  const hex2 = (b: number | null) => (b === null ? "··" : b.toString(16).padStart(2, "0"));

  /** The server prints floats as full decimal expansions; show the shortest text that reads back as the same number. */
  function floatText(text: string | null, bits: 32 | 64): string {
    if (text === null) return "–";
    const n = Number(text);
    if (!Number.isFinite(n)) return text;
    if (n === 0) return text.startsWith("-") ? "-0" : "0";
    if (bits === 64) return String(n);
    for (let p = 1; p <= 9; p++) {
      const t = Number(n.toPrecision(p));
      if (Math.fround(t) === Math.fround(n)) return String(t);
    }
    return String(n);
  }

  /** Rust's `{:?}` writes control characters as `\u{4}`; C readers expect `\x04`. */
  const cText = (s: string) => s.replace(/\\u\{([0-9a-f]{1,2})\}/gi, (_, h: string) => `\\x${h.padStart(2, "0")}`);
</script>

{#snippet change(r: NavResult | undefined, dir: "prev" | "next")}
  {@const name = dir === "prev" ? "◀ previous change" : "next change"}
  {#if r === undefined}
    <div class="dim line">{name}: …</div>
  {:else if r.step === null}
    <div class="dim line">{name}: none</div>
  {:else}
    {@const k = r.step}
    {@const where = [`step ${k}`, r.loc, r.asm].filter(Boolean).join(" · ")}
    <button class="link line" onclick={() => lab.goto(k, { record: true })} title="Go to step {k}">{name}: {where}{dir === "next" ? " ▶" : ""}</button>
  {/if}
{/snippet}

<div class="panel">
  <h2>
    <span>Inspector</span>
    <button class="x" onclick={() => lab.select(null)} title="Clear the selection (Esc)" aria-label="Clear the selection">×</button>
  </h2>
  <div class="body">
    {#if sel}
      <div class="head mono">
        {#if sel.kind === "reg"}
          <b>%{sel.name}</b>
          {#if renderedNote}<span class="dim">{renderedNote}</span>{/if}
        {:else}
          <b>{mem?.rendered ?? sel.addr}</b>
          <span class="dim">{mem ? `${sel.addr} · ` : ""}{sel.size} byte{sel.size === 1 ? "" : "s"}</span>
          <button class="show" onclick={() => lab.showInMemory(sel.addr)} title="Open the Memory tab at this address">Show in memory</button>
        {/if}
      </div>
      <div class="nav">
        {@render change(probe?.prev, "prev")}
        {@render change(probe?.next, "next")}
      </div>

      {#if sel.kind === "reg" && reg}
        {#if reg.name === "rflags"}
          <table class="tbl flagtbl">
            <tbody>
              {#each FLAGS as f (f.name)}
                <tr class:changed={flagChanged(val, old, f.bit)}>
                  <td class="mono">{f.name}</td>
                  <td class="mono">{(val >> BigInt(f.bit)) & 1n}</td>
                  <td class="dim">{f.meaning}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {:else}
          <table class="tbl mono">
            <thead>
              <tr class="dim"><th></th><th>hex</th><th>unsigned</th><th>signed</th></tr>
            </thead>
            <tbody>
              {#each parts as p (p.name)}
                {@const now = piece(val, p)}
                <tr class:changed={old !== null && piece(old, p) !== now}>
                  <td>%{p.name}</td>
                  <td>{hex(now, p.bits / 8)}</td>
                  <td>{now}</td>
                  <td>{signed(now, p.bits)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
          <div class="bin mono" title="The 64 bits of %{reg.name}, high byte first. Highlighted bits differ from the previous step.">
            {#each BYTE_GROUPS as half (half)}
              <div>
                <span class="dim range">{half === 0 ? "63–32" : "31–0"}</span>
                {#each bitsNow.slice(half * 4, half * 4 + 4) as g, gi (gi)}
                  <span class="byte">
                    {#each [...g] as bit, bi (bi)}<span class:one={bit === "1"} class:diff={bitsOld !== null && bitsOld[half * 4 + gi][bi] !== bit}>{bit}</span>{/each}
                  </span>
                {/each}
              </div>
            {/each}
          </div>
          {#if isGpr}
            <p class="dim note">Writing a 32-bit register (e.g. %eax) zero-extends into the full 64-bit register; 8- and 16-bit writes leave the other bits unchanged.</p>
          {/if}
        {/if}
      {:else if sel.kind === "mem"}
        {#if mem}
          <table class="tbl mono">
            <thead>
              <tr class="dim"><th></th><th>unsigned</th><th>signed</th></tr>
            </thead>
            <tbody>
              {#each INT_ROWS as [bits, u, s] (bits)}
                <tr class:now={sel.size === bits / 8}>
                  <td class="dim">{bits}-bit</td>
                  <td>{byType.get(u) ?? "–"}</td>
                  <td>{byType.get(s) ?? "–"}</td>
                </tr>
              {/each}
              <tr class:now={sel.size === 4}><td class="dim">float</td><td colspan="2">{floatText(byType.get("f32") ?? null, 32)}</td></tr>
              <tr class:now={sel.size === 8}><td class="dim">double</td><td colspan="2">{floatText(byType.get("f64") ?? null, 64)}</td></tr>
              <tr><td class="dim">pointer</td><td colspan="2">{byType.get("ptr") ?? "–"}</td></tr>
            </tbody>
          </table>
          <div class="kv">
            <span class="dim">written</span>
            {#if mem.writer === null}
              <span>initial contents</span>
            {:else}
              {@const w = mem.writer}
              <span>
                last written at step <button class="link" onclick={() => lab.goto(w, { record: true })} title="Go to step {w}">{w}</button>{mem.writer_asm ? " by " : ""}<span class="mono">{mem.writer_asm ?? ""}</span>
              </span>
            {/if}
          </div>
          <div class="kv"><span class="dim">region</span><span class="mono">{mem.region}</span></div>
          {#if mem.cstring !== null}
            <div class="kv"><span class="dim">string</span><span class="mono">{cText(mem.cstring)}</span></div>
          {/if}
          <div class="kv"><span class="dim">bytes</span><span class="mono">{mem.bytes.map(hex2).join(" ")}</span><span class="dim">lowest address first</span></div>
        {:else}
          <p class="dim note">Loading…</p>
        {/if}
      {/if}
    {/if}
  </div>
</div>

<style>
  .panel > h2 { display: flex; align-items: center; justify-content: space-between; }
  .x { border: none; background: none; padding: 0 4px; line-height: 1; font-size: 14px; color: var(--dim); }
  .x:hover:not(:disabled) { color: var(--fg); }
  .body { padding: 4px 8px; }
  .head { display: flex; flex-wrap: wrap; gap: 2px 10px; align-items: baseline; margin-bottom: 2px; }
  .nav { margin-bottom: 4px; }
  .line { display: block; width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; }
  .link { border: none; background: none; padding: 0; color: var(--accent); cursor: pointer; }
  .link:hover:not(:disabled) { text-decoration: underline; }
  .tbl { border-collapse: collapse; margin: 2px 0 4px; }
  .tbl th { font-weight: 400; text-align: left; padding: 0 12px 0 0; }
  .tbl td { padding: 0 12px 0 0; white-space: nowrap; }
  .flagtbl td { white-space: normal; }
  tr.changed { background: var(--changed); }
  tr.now { background: var(--cur); }
  .bin { margin: 2px 0; line-height: 1.5; }
  .range { display: inline-block; width: 6ch; }
  .byte { margin-right: 1ch; }
  .byte span { color: var(--dim); }
  .byte span.one { color: var(--fg); font-weight: 700; }
  .byte span.diff { background: var(--changed); }
  .note { margin: 4px 0 0; }
  .kv { display: flex; flex-wrap: wrap; gap: 0 10px; margin: 2px 0; }
  .kv > :first-child { min-width: 7ch; }
  .show { margin-left: auto; padding: 0 6px; font-size: 12px; }
</style>
