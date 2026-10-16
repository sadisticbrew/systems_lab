<script lang="ts">
  import { api, type Watch, type WatchType } from "../lib/api";
  import { lab } from "../lib/state.svelte";

  const TYPES: WatchType[] = ["value", "u8", "i8", "u16", "i16", "u32", "i32", "u64", "i64", "f32", "f64", "ptr", "str", "bytes"];
  /** Bytes a watch of this type covers, for selecting its address. */
  const WIDTH: Record<WatchType, number> = {
    value: 8, u8: 1, i8: 1, u16: 2, i16: 2, u32: 4, i32: 4, u64: 8, i64: 8, f32: 4, f64: 8, ptr: 8, str: 1, bytes: 1,
  };

  const list = $derived(lab.notes.watches ?? []);
  let rows = $state.raw<Watch[]>([]);
  let text = $state("");
  let error = $state("");
  let adding = $state(false);

  // Refetch when the step, the list or the recording changes; an answer for an older request is dropped.
  $effect(() => {
    const req = list.map((w) => ({ expr: w.expr, type: w.type }));
    const step = lab.step;
    void lab.status.generation;
    if (req.length === 0) {
      rows = [];
      return;
    }
    const ctl = new AbortController();
    api.watch(step, req, ctl.signal).then(
      (r) => (rows = r.watches),
      (e) => lab.fail(e),
    );
    return () => ctl.abort();
  });

  /** The answer for `w`, unless it was computed for an older version of the row. */
  const answer = (w: { expr: string; type: WatchType }, i: number): Watch | null => {
    const r = rows[i];
    return r && r.expr === w.expr && r.type === w.type ? r : null;
  };

  async function add() {
    const expr = text.trim();
    if (expr === "" || adding) return;
    adding = true;
    error = "";
    try {
      const { default_type } = await api.eval(expr, lab.step);
      const have = lab.notes.watches ?? [];
      if (!have.some((w) => w.expr === expr && w.type === default_type)) {
        lab.notes.watches = [...have, { expr, type: default_type }];
        lab.saveNotes();
      }
      text = "";
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      adding = false;
    }
  }

  function remove(w: object) {
    lab.notes.watches = (lab.notes.watches ?? []).filter((x) => x !== w);
    lab.saveNotes();
  }

  function retype(w: { type: WatchType }, type: string) {
    w.type = type as WatchType;
    lab.saveNotes();
  }

  /** A bare register name selects that register; anything else with an address selects the bytes it reads. */
  function pick(w: { expr: string; type: WatchType }, r: Watch | null) {
    const name = w.expr.trim().replace(/^%/, "").toLowerCase();
    if (lab.cur?.regs.some((x) => x.name === name)) lab.select({ kind: "reg", name });
    else if (r?.addr) lab.select({ kind: "mem", addr: r.addr, size: WIDTH[w.type] });
  }

  const valueTip = (r: Watch | null) =>
    r ? [r.addr && `at ${r.addr}`, r.raw && `raw ${r.raw}`, r.prev !== null && `previous: ${r.prev}`].filter(Boolean).join("\n") : "";
</script>

<div class="panel">
  <h2>Watch</h2>
  <div class="body">
    {#if list.length === 0}
      <p class="dim empty">Add a register (rax), a variable (n) or an address (rbp-0x10).</p>
    {:else}
      <div class="list mono">
        {#each list as w, i (w)}
          {@const r = answer(w, i)}
          <div class="row" class:changed={r?.changed}>
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <span class="expr clickable" onclick={() => pick(w, r)} title={w.expr}>{w.expr}</span>
            <select value={w.type} onchange={(e) => retype(w, e.currentTarget.value)} title="How the value is read" aria-label="Type of {w.expr}">
              {#each TYPES as t (t)}<option value={t}>{t}</option>{/each}
            </select>
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <span class="val clickable" onclick={() => pick(w, r)} title={valueTip(r)}>
              {#if r?.error}<span class="err">{r.error}</span>{:else if r?.value != null}{r.value}{:else if r}<span class="dim">unmapped at {r.addr}</span>{/if}
            </span>
            <button class="x" onclick={() => remove(w)} title="Stop watching {w.expr}" aria-label="Stop watching {w.expr}">×</button>
          </div>
        {/each}
      </div>
    {/if}
  </div>
  <div class="add">
    <input
      class="mono"
      bind:value={text}
      placeholder="watch an expression, then Enter"
      spellcheck="false"
      autocomplete="off"
      onkeydown={(e) => e.key === "Enter" && add()}
      oninput={() => (error = "")}
    />
    {#if error}<div class="err small">{error}</div>{/if}
  </div>
</div>

<style>
  .empty { margin: 0; padding: 4px 8px; }
  .list { display: grid; grid-template-columns: minmax(3ch, 14ch) auto minmax(0, 1fr) auto; }
  .row { display: grid; grid-template-columns: subgrid; grid-column: 1 / -1; align-items: center; gap: 0 8px; padding: 0 4px 0 8px; border-bottom: 1px solid var(--border); }
  .row:last-child { border-bottom: none; }
  .row.changed { background: var(--changed); }
  .expr { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .val { overflow-wrap: anywhere; min-width: 0; }
  select { font-size: 11px; padding: 0 2px; }
  .x { border: none; background: none; padding: 0 4px; line-height: 1; font-size: 14px; color: var(--dim); }
  .x:hover:not(:disabled) { color: var(--fg); }
  .err { color: var(--red); }
  .add { flex: none; padding: 3px 8px; border-top: 1px solid var(--border); }
  .add input { width: 100%; }
  .small { font-size: 11px; margin-top: 2px; }
</style>
