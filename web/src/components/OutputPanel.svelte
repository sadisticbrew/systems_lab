<script lang="ts">
  import type { EventDetail } from "../lib/api";
  import { lab } from "../lib/state.svelte";

  type Chunk = { step: number; fd: number; text: string | null; bytes: number };

  /** Every `write` to stdout or stderr that returned a positive count, in order. The server limits `buf` to the bytes the kernel accepted. */
  function chunksOf(events: EventDetail[]): Chunk[] {
    const out: Chunk[] = [];
    for (const e of events) {
      if (e.kind !== "syscall" || e.name !== "write" || e.ret === null) continue;
      const fd = Number(e.args.find((a) => a.name === "fd")?.value);
      const bytes = Number(e.ret);
      if ((fd !== 1 && fd !== 2) || !(bytes > 0)) continue;
      out.push({ step: e.step, fd, text: e.buffers.find((b) => b.arg === "buf")?.text ?? null, bytes });
    }
    return out;
  }

  const chunks = $derived(chunksOf(lab.events));
  // Step N is the state before instruction N runs, so a write at step N has happened from state N + 1 on.
  const shown = $derived(chunks.filter((c) => c.step < lab.step));
  const later = $derived(chunks.slice(shown.length).reduce((n, c) => n + c.bytes, 0));
</script>

<div class="out">
  {#if shown.length}
    <pre class="mono">{#each shown as c}<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions --><span class="chunk clickable" class:err={c.fd === 2} title="step {c.step}" onclick={() => lab.goto(c.step, { record: true })}>{c.text ?? `[${c.bytes} bytes could not be read]`}</span>{/each}</pre>
  {:else}
    <div class="dim empty">No output yet.</div>
  {/if}
  {#if later > 0}
    <div class="dim more">{later} more {later === 1 ? "byte is" : "bytes are"} written later</div>
  {/if}
</div>

<style>
  .out { padding: 6px 10px; }
  pre { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
  .err { color: var(--red); }
  .empty, .more { font-size: 12px; }
  .more { margin-top: 6px; }
</style>
