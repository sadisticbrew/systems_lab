<script lang="ts">
  import { untrack } from "svelte";
  import type { EventDetail } from "../lib/api";
  import { escapeBytes } from "../lib/format";
  import { lab } from "../lib/state.svelte";

  type Syscall = Extract<EventDetail, { kind: "syscall" }>;

  const MAX_BUF = 24;

  /** Index of the row the learner clicked; dropped as soon as the displayed step moves elsewhere. */
  let picked = $state<number | null>(null);
  let listEl = $state<HTMLDivElement>();

  const latest = $derived.by(() => {
    let found = -1;
    lab.events.forEach((e, i) => {
      if (e.step <= lab.step) found = i;
    });
    return found;
  });
  const sel = $derived(picked !== null && picked < lab.events.length ? picked : latest);
  const ev = $derived(sel >= 0 ? lab.events[sel] : null);

  $effect(() => {
    const step = lab.step;
    untrack(() => {
      if (picked !== null && lab.events[picked]?.step !== step) picked = null;
    });
  });

  // Keep the highlighted row in view while stepping.
  $effect(() => {
    const row = listEl?.querySelector<HTMLElement>(`[data-i="${sel}"]`);
    if (!listEl || !row) return;
    if (row.offsetTop < listEl.scrollTop) listEl.scrollTop = row.offsetTop;
    else if (row.offsetTop + row.offsetHeight > listEl.scrollTop + listEl.clientHeight) listEl.scrollTop = row.offsetTop + row.offsetHeight - listEl.clientHeight;
  });

  function pick(i: number) {
    picked = i;
    lab.goto(lab.events[i].step, { record: true });
  }

  const quoted = (s: string) => `"${s}"`;

  /** The text of the buffer an argument points to, escaped and cut for a one-line row. */
  function bufferText(e: Syscall, arg: string): string | null {
    const b = e.buffers.find((x) => x.arg === arg);
    if (!b || b.text === null) return null;
    const t = escapeBytes(b.text);
    return quoted(t.length > MAX_BUF ? `${t.slice(0, MAX_BUF)}…` : t);
  }

  function call(e: Syscall): string {
    return `${e.name}(${e.args.map((a) => bufferText(e, a.name) ?? a.value).join(", ")})`;
  }
</script>

<div class="events">
  <div class="list mono" bind:this={listEl}>
    {#each lab.events as e, i}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="item clickable" class:cur={i === sel} class:later={e.step > lab.step} data-i={i} onclick={() => pick(i)}>
        <span class="step dim">#{e.step}</span>
        <span class="k {e.kind}">{e.kind}</span>
        {#if e.kind === "syscall"}
          <span class="text">{call(e)}{e.ret !== null ? ` = ${e.ret}` : ""}{e.errno ? ` ${e.errno}` : ""}</span>
        {:else}
          <span class="text">{e.signal}{e.code_name ? ` (${e.code_name})` : ""}</span>
        {/if}
      </div>
    {:else}
      <div class="dim pad">No system calls or signals were recorded.</div>
    {/each}
  </div>
  {#if ev}
    <div class="detail">
      <div class="head"><b>{ev.kind === "syscall" ? ev.name : ev.signal}</b> <span class="dim">at step {ev.step}</span></div>
      {#if ev.kind === "syscall"}
        <div class="mono row"><span class="reg">%rax</span> = {ev.nr} <span class="dim">({ev.name})</span></div>
        {#each ev.args as a}
          <div class="mono row">
            <span class="reg">%{a.reg}</span>
            <span class="argname">{a.name}</span> = <span>{a.value}</span>
            {#if a.note}<span class="dim">({a.note})</span>{/if}
          </div>
        {/each}
        <div class="mono row ret">
          {#if ev.ret === null}
            <span class="dim">no return value was recorded</span>
          {:else}
            returns <b>{ev.ret}</b>
            {#if ev.errno}<span class="err">errno: {ev.errno}{ev.errno_text ? ` (${ev.errno_text})` : ""}</span>{/if}
          {/if}
        </div>
        {#each ev.buffers as b}
          <div class="buf">
            <div class="dim">{b.arg} · {b.dir === "in" ? "read by the kernel" : "filled in by the kernel"} · {b.len} {b.len === 1 ? "byte" : "bytes"}{b.truncated ? " · truncated" : ""}</div>
            {#if b.text !== null}<div class="mono text">{quoted(escapeBytes(b.text))}</div>{:else}<div class="dim">not readable at this step</div>{/if}
          </div>
        {/each}
      {:else}
        <div class="mono row">code {ev.code}{ev.code_name ? ` ${ev.code_name}` : ""}{ev.code_text ? ` — ${ev.code_text}` : ""}</div>
        <div class="mono row">address {ev.addr}</div>
        {#if ev.cpu_exception}<div class="mono row">CPU exception: {ev.cpu_exception}</div>{/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  .events { display: flex; height: 100%; min-height: 0; }
  .list { flex: 1 1 0; min-width: 0; overflow: auto; position: relative; }
  .detail { flex: 0 1 46%; min-width: 220px; overflow: auto; border-left: 1px solid var(--border); padding: 4px 10px; }
  .pad { padding: 8px; font-family: var(--font); }
  .item { display: flex; gap: 12px; padding: 1px 8px; white-space: pre; }
  .item.cur { background: var(--hl); }
  .item.later { opacity: 0.5; }
  .step { min-width: 8ch; text-align: right; }
  .k { min-width: 7ch; font-weight: 700; }
  .syscall { color: var(--c3); }
  .signal { color: var(--red); }
  .head { margin-bottom: 2px; }
  .row { padding: 1px 0; white-space: pre-wrap; overflow-wrap: anywhere; }
  .reg { color: var(--c3); display: inline-block; min-width: 5ch; }
  .argname { color: var(--dim); }
  .ret { margin-top: 2px; }
  .err { color: var(--red); margin-left: 8px; }
  .buf { margin-top: 4px; }
  .buf .text { white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
