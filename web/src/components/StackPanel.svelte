<script lang="ts">
  import type { Frame, Slot } from "../lib/api";
  import { big, mod16 } from "../lib/format";
  import { lab } from "../lib/state.svelte";

  const cur = $derived(lab.cur);
  const stack = $derived(cur?.stack ?? null);
  const rsp = $derived(cur?.regs.find((r) => r.name === "rsp") ?? null);
  const rspMod = $derived(rsp ? mod16(rsp.value) : null);
  const aboutToCall = $derived(cur?.effects?.control?.kind === "call");
  /** Bytes of the selected memory, as `[lo, hi)`. */
  const chosen = $derived.by(() => {
    const s = lab.selection;
    return s?.kind === "mem" ? { lo: big(s.addr), hi: big(s.addr) + BigInt(Math.max(1, s.size)) } : null;
  });

  /** The placeholder the server puts in place of the middle of a very large frame. */
  const isGap = (s: Slot) => s.raw_value === "";

  function isChosen(s: Slot): boolean {
    if (!chosen) return false;
    const a = big(s.raw_addr);
    return a < chosen.hi && chosen.lo < a + 8n;
  }

  const frameTip = (f: Frame) =>
    f.call_step === null
      ? ""
      : `created by the call at step ${f.call_step}; ${f.ret_step === null ? "does not return in this recording" : `returns at step ${f.ret_step}`}`;

  const slotTip = (s: Slot) =>
    `${s.label}\n${s.raw_addr}: ${s.writer === null ? "initial contents" : `last written at step ${s.writer}`}${s.label === "unused" ? `\nvalue ${s.value}` : ""}`;

  function setAddrs(on: boolean) {
    lab.prefs.stackAddrs = on;
    lab.savePrefs();
  }
</script>

<div class="panel">
  <h2>
    <span>Stack</span>
    {#if stack}
      <span class="meta mono">· {stack.loc} · rsp = {rsp?.rendered ?? "?"} · rsp mod 16 = {rspMod ?? "?"}</span>
      {#if aboutToCall && rspMod !== null}
        <span
          class="badge"
          class:ok={rspMod === 0}
          class:bad={rspMod !== 0}
          title={rspMod === 0
            ? "System V requires %rsp to be a multiple of 16 when a call runs, and it is."
            : `System V requires %rsp to be a multiple of 16 when a call runs; here %rsp mod 16 = ${rspMod}.`}
        >{rspMod === 0 ? "✓ aligned" : "✗ misaligned"}</span>
      {/if}
    {/if}
    <label class="addrs"><input type="checkbox" checked={lab.prefs.stackAddrs} onchange={(e) => setAddrs(e.currentTarget.checked)} /> addresses</label>
  </h2>
  <div class="body mono">
    {#if stack}
      {@const cols = lab.prefs.stackAddrs ? 5 : 4}
      <div class="edge dim">↑ higher addresses</div>
      <table class="stk">
        {#each stack.frames as f, fi (fi)}
          {@const title = f.collapsed ? `${f.label}'s frame` : `${f.label}, ${f.size ?? 0} bytes`}
          {#if fi > 0}<tbody class="sp"><tr><td colspan={cols}></td></tr></tbody>{/if}
          <tbody class="frame">
            {#if f.call_step !== null}
              {@const call = f.call_step}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
              <tr class="head clickable" class:dim={f.collapsed} onclick={() => lab.goto(call, { record: true })} title={frameTip(f)}>
                <th colspan={cols}>{title}</th>
              </tr>
            {:else}
              <tr class="head" class:dim={f.collapsed}><th colspan={cols}>{title}</th></tr>
            {/if}
            {#each f.slots as s, si (si)}
              {#if isGap(s)}
                <tr class="gap dim" title={s.label}><td colspan={cols}>{s.label}</td></tr>
              {:else}
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
                <tr
                  class="row clickable"
                  class:unused={s.label === "unused"}
                  class:sel={isChosen(s)}
                  onclick={() => lab.select({ kind: "mem", addr: s.raw_addr, size: 8 })}
                  ondblclick={() => lab.nav("write-prev", { addr: s.raw_addr })}
                  title={slotTip(s)}
                >
                  {#if lab.prefs.stackAddrs}<td class="addr dim" title={s.raw_addr}>{s.addr}</td>{/if}
                  <td class="off dim">{s.rbp_off ?? ""}</td>
                  <td class="lbl"><span class="clamp">{s.label}</span></td>
                  <td class="val" title="{s.value} ({s.raw_value})"><span class="cut">{s.label === "unused" ? "-" : s.value}</span></td>
                  <td class="ptr">{s.pointers.length ? "← " + s.pointers.join(", ") : ""}</td>
                </tr>
              {/if}
            {/each}
          </tbody>
        {/each}
      </table>
      <div class="edge dim">↓ lower addresses — the stack grows down</div>
    {/if}
  </div>
</div>

<style>
  .panel > h2 { display: flex; align-items: center; gap: 6px; }
  .meta { text-transform: none; letter-spacing: 0; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .badge { flex: none; text-transform: none; letter-spacing: 0; font-weight: 700; }
  .badge.ok { color: var(--c1); }
  .badge.bad { color: var(--red); }
  .addrs { flex: none; margin-left: auto; text-transform: none; letter-spacing: 0; font-weight: 400; cursor: pointer; }
  .addrs input { vertical-align: middle; margin: 0 2px 0 0; }
  .edge { padding: 2px 10px; font-size: 11px; }
  .stk { width: calc(100% - 12px); margin: 2px 6px; border-collapse: separate; border-spacing: 0; }
  .stk td, .stk th { padding: 0 4px; text-align: left; vertical-align: top; }
  .sp td { height: 8px; padding: 0; }
  .frame { outline: 1px solid var(--border); outline-offset: -1px; border-radius: 4px; }
  .head th { padding: 2px 8px; background: var(--bg); border-bottom: 1px solid var(--border); font-weight: 600; }
  .head.dim th { font-weight: 400; }
  .frame > :last-child td { border-bottom: none; }
  .row td, .gap td { border-bottom: 1px solid var(--border); }
  .row td:first-child, .gap td { padding-left: 8px; }
  .row td:last-child { padding-right: 8px; }
  .gap td { font-style: italic; }
  .row.sel { outline: 1px solid var(--accent); outline-offset: -1px; }
  .stk td.off { text-align: right; white-space: nowrap; }
  .addr, .val, .ptr { white-space: nowrap; }
  .lbl { width: 100%; min-width: 18ch; }
  .clamp { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; overflow-wrap: anywhere; }
  /* Long symbolic values (`ld-linux-x86-64.so.2:_rtld_global`) give way to the label; the tooltip has the raw value. */
  .cut { display: block; max-width: 20ch; overflow: hidden; text-overflow: ellipsis; }
  .unused { color: var(--dim); }
  .ptr { color: var(--accent); font-weight: 700; }
</style>
