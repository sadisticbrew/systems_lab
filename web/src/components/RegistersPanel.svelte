<script lang="ts">
  import { lab } from "../lib/state.svelte";

  let showXmm = $state(false);
  const cur = $derived(lab.cur);
  const flagOrder = ["CF", "PF", "AF", "ZF", "SF", "TF", "IF", "DF", "OF"];
</script>

<div class="panel">
  <h2>Registers</h2>
  <div class="body mono">
    {#if cur}
      <table>
        <tbody>
          {#each cur.regs as r (r.name)}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <tr class="clickable" class:changed={r.changed} onclick={() => lab.nav("reg-prev", { reg: r.name })} title="jump to the step that last changed {r.name}">
              <td class="name">{r.name}</td>
              <td>{r.value}</td>
              <td class="dim">{r.rendered}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      <div class="flags">
        {#each flagOrder as f}
          <span class:on={cur.flags[f]} title={f}>{f}={cur.flags[f] ?? 0}</span>
        {/each}
      </div>
      <button class="xmmbtn" onclick={() => (showXmm = !showXmm)}>{showXmm ? "▾" : "▸"} xmm</button>
      {#if showXmm}
        <table>
          <tbody>
            {#each cur.xmm as x (x.name)}
              <tr class:changed={x.changed}><td class="name">{x.name}</td><td>{x.value}</td></tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {/if}
  </div>
</div>

<style>
  table { border-collapse: collapse; width: 100%; }
  td { padding: 0 6px; white-space: nowrap; }
  .name { width: 7ch; }
  tr.changed { background: var(--changed); }
  tr.changed:hover { background: var(--cur); }
  .flags { display: flex; flex-wrap: wrap; gap: 8px; padding: 4px 6px; border-top: 1px solid var(--border); }
  .flags span { color: var(--dim); }
  .flags span.on { color: var(--fg); font-weight: 700; }
  .xmmbtn { margin: 4px 6px; }
</style>
