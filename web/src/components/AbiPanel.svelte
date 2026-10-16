<script lang="ts">
  import { untrack } from "svelte";
  import { ABI_RULES } from "../lib/abi";
  import { lab } from "../lib/state.svelte";

  /** Index of the finding the learner clicked; dropped as soon as the displayed step moves elsewhere. */
  let picked = $state<number | null>(null);

  const sel = $derived(picked !== null && picked < lab.findings.length ? picked : lab.findings.findIndex((f) => f.step === lab.step));

  $effect(() => {
    const step = lab.step;
    untrack(() => {
      if (picked !== null && lab.findings[picked]?.step !== step) picked = null;
    });
  });

  function pick(i: number) {
    picked = i;
    lab.goto(lab.findings[i].step, { record: true });
  }
</script>

<div class="list">
  {#each lab.findings as f, i}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="item clickable mono" class:cur={i === sel} onclick={() => pick(i)}>
      <span class="step dim">#{f.step}</span><span class="k abi">{f.rule}</span><span class="dim">{f.loc}</span><span>{f.message}</span>
    </div>
    {#if i === sel}
      <div class="why">{ABI_RULES[f.rule] ?? "This rule has no written explanation yet."}</div>
    {/if}
  {:else}
    <div class="dim pad">No ABI findings: every call and return followed the rules this viewer checks.</div>
  {/each}
  {#if lab.findings.length && sel < 0}
    <div class="dim pad">Select a finding to see why the rule exists.</div>
  {/if}
</div>

<style>
  .pad { padding: 8px; }
  .item { display: flex; gap: 12px; padding: 1px 8px; white-space: pre; }
  .cur { background: var(--hl); }
  .step { min-width: 8ch; text-align: right; }
  .k { min-width: 10ch; font-weight: 700; }
  .abi { color: var(--orange); }
  .why { padding: 4px 12px 6px 28px; max-width: 90ch; }
</style>
