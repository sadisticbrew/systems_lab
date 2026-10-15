<script lang="ts">
  import { lab } from "../lib/state.svelte";

  const stack = $derived(lab.cur?.stack ?? null);
</script>

<div class="panel">
  <h2>Stack {stack ? `· ${stack.loc}` : ""}</h2>
  <div class="body mono">
    {#if stack}
      {#each stack.frames as f}
        <div class="frame">
          {#if f.collapsed}
            <div class="head dim">{f.label}'s frame</div>
          {:else}
            <div class="head">{f.label}, {f.size ?? 0} bytes</div>
            {#each f.slots as s (s.raw_addr)}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <div
                class="row clickable"
                class:unused={s.label === "unused"}
                onclick={() => lab.nav("write-prev", { addr: s.raw_addr })}
                title={s.writer === null ? `${s.raw_addr}: from the initial snapshot` : `${s.raw_addr}: last written at step ${s.writer}`}
              >
                <span class="off dim">{s.rbp_off ?? ""}</span>
                <span class="lbl">{s.label}</span>
                <span class="val">{s.label === "unused" ? "-" : s.value}</span>
                <span class="ptr">{s.pointers.length ? "← " + s.pointers.join(", ") : ""}</span>
              </div>
            {/each}
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .frame { margin: 6px; border: 1px solid var(--border); border-radius: 4px; overflow: hidden; }
  .head { padding: 2px 8px; background: var(--bg); border-bottom: 1px solid var(--border); font-weight: 600; }
  .frame > .head:only-child { border-bottom: none; font-weight: 400; }
  .row { display: grid; grid-template-columns: 10ch minmax(0, 1fr) minmax(8ch, auto) 9ch; gap: 8px; padding: 0 8px; border-bottom: 1px solid var(--border); }
  .row:last-child { border-bottom: none; }
  .off { text-align: right; }
  .lbl { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .unused { color: var(--dim); }
  .ptr { color: var(--accent); font-weight: 700; white-space: nowrap; }
</style>
