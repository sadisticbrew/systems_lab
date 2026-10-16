<script lang="ts">
  import { untrack } from "svelte";
  import { api, type Mem } from "../lib/api";
  import { lab } from "../lib/state.svelte";

  const LEN = 256;
  let input = $state("");
  let mem = $state<Mem | null>(null);
  let note = $state("");
  let ctl: AbortController | null = null;

  // Effective base address as bigint, or null when it cannot be resolved.
  const base = $derived.by<bigint | null>(() => {
    const regs = lab.cur?.regs;
    if (!regs) return null;
    const t = input.trim().replace(/^%/, "").toLowerCase();
    if (t === "") {
      const rsp = regs.find((r) => r.name === "rsp");
      return rsp ? BigInt(rsp.value) & ~15n : null;
    }
    const reg = regs.find((r) => r.name === t);
    if (reg) return BigInt(reg.value);
    if (/^(0x)?[0-9a-f]+$/.test(t)) return BigInt(t.startsWith("0x") ? t : "0x" + t);
    return null;
  });

  $effect(() => {
    const b = base;
    const step = lab.step;
    if (b === null) {
      note = lab.cur ? `cannot resolve "${input}"` : "";
      return;
    }
    note = "";
    untrack(() => {
      ctl?.abort();
      const c = new AbortController();
      ctl = c;
      api.mem("0x" + b.toString(16), LEN, step, c.signal)
        .then((m) => { if (ctl === c) mem = m; })
        .catch((e) => lab.fail(e));
    });
  });

  const written = $derived.by(() => {
    const set = new Set<string>();
    if (!mem) return set;
    const start = BigInt(mem.addr);
    for (const w of lab.cur?.prev_writes ?? []) {
      const a = BigInt(w.raw_addr);
      for (let i = 0n; i < BigInt(w.size); i++) {
        const off = a + i - start;
        if (off >= 0n && off < BigInt(LEN)) set.add(off.toString());
      }
    }
    return set;
  });

  const hex2 = (n: number) => n.toString(16).padStart(2, "0");
  const ascii = (n: number | null) => (n === null ? "·" : n >= 32 && n < 127 ? String.fromCharCode(n) : ".");
  const rowAddr = (r: number) => (mem ? "0x" + (BigInt(mem.addr) + BigInt(r * 16)).toString(16) : "");
  const byteAddr = (i: number) => (mem ? "0x" + (BigInt(mem.addr) + BigInt(i)).toString(16) : "");
</script>

<div class="mem mono">
  <div class="bar">
    <label class="dim">address
      <input bind:value={input} placeholder="following rsp (hex or register)" spellcheck="false" />
    </label>
    {#if note}<span class="warn">{note}</span>{/if}
    {#if input}<button onclick={() => (input = "")}>follow rsp</button>{/if}
  </div>
  {#if mem}
    <div class="grid">
      {#each Array(16) as _, r}
        <div class="row">
          <span class="a dim">{rowAddr(r)}</span>
          <span class="bytes">
            {#each Array(16) as _, c}
              {@const i = r * 16 + c}
              {@const b = mem.bytes[i] ?? null}
              {@const w = mem.writers[i] ?? null}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <span
                class="b clickable"
                class:hot={written.has(String(i))}
                class:nul={b === null}
                onclick={() => lab.nav("write-prev", { addr: byteAddr(i) })}
                title={b === null ? `${byteAddr(i)}: unmapped` : `${byteAddr(i)}: ${w === null ? "initial snapshot" : "written at step " + w}`}
              >{b === null ? "··" : hex2(b)}</span>
            {/each}
          </span>
          <span class="asc dim">{#each Array(16) as _, c}{ascii(mem.bytes[r * 16 + c] ?? null)}{/each}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .mem { padding: 4px 8px; }
  .bar { display: flex; gap: 10px; align-items: center; margin-bottom: 4px; font-family: var(--font); }
  .bar input { width: 260px; font-family: var(--mono); }
  .warn { color: var(--red); }
  .row { display: flex; gap: 14px; white-space: pre; }
  .a { min-width: 14ch; }
  .bytes { display: inline-flex; gap: 6px; }
  .b { padding: 0 1px; border-radius: 2px; }
  .b.hot { background: var(--changed); font-weight: 700; }
  .nul { color: var(--dim); }
</style>
