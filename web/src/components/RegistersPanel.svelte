<script lang="ts">
  import { FLAGS, flagChanged } from "../lib/flags";
  import { big } from "../lib/format";
  import { lab } from "../lib/state.svelte";

  let showXmm = $state(false);
  const cur = $derived(lab.cur);
  const selected = $derived(lab.selection?.kind === "reg" ? lab.selection.name : null);
  const rflags = $derived(cur?.regs.find((r) => r.name === "rflags") ?? null);
  const flagValue = $derived(rflags ? big(rflags.value) : 0n);
  const flagPrev = $derived(rflags?.prev != null ? big(rflags.prev) : null);
</script>

<div class="panel">
  <h2>Registers</h2>
  <div class="body mono">
    {#if cur}
      <table>
        <tbody>
          {#each cur.regs as r (r.name)}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
            <tr
              class="clickable"
              class:changed={r.changed && r.name !== "rip"}
              class:sel={selected === r.name}
              onclick={() => lab.select({ kind: "reg", name: r.name })}
              ondblclick={() => lab.nav("reg-prev", { reg: r.name })}
              title="Click to inspect %{r.name}. Double-click to go to the step that last changed it."
            >
              <td class="name">{r.name}</td>
              <td>{r.value}</td>
              <td class="dim">{r.rendered}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="flags" class:sel={selected === "rflags"} onclick={() => lab.select({ kind: "reg", name: "rflags" })} title="Click to inspect %rflags">
        {#each FLAGS as f (f.name)}
          <span class:on={cur.flags[f.name]} class:changed={flagChanged(flagValue, flagPrev, f.bit)} title="{f.name}: {f.meaning}">{f.name}={cur.flags[f.name] ?? 0}</span>
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
  tr.sel { outline: 1px solid var(--accent); outline-offset: -1px; }
  .flags { display: flex; flex-wrap: wrap; gap: 4px 8px; padding: 4px 6px; border-top: 1px solid var(--border); cursor: pointer; }
  .flags.sel { outline: 1px solid var(--accent); outline-offset: -1px; }
  .flags span { color: var(--dim); padding: 0 2px; border-radius: 2px; }
  .flags span.on { color: var(--fg); font-weight: 700; }
  .flags span.changed { background: var(--changed); }
  .xmmbtn { margin: 4px 6px; }
</style>
