<script lang="ts">
  import { onMount } from "svelte";
  import { lab } from "../lib/state.svelte";

  const concepts = [
    "Step N is the machine state just before instruction N runs. The Effects panel shows what that instruction is about to do.",
    "Highlighted registers and filled memory bytes were changed by the previous instruction; outlined bytes are about to be written.",
    "Click a register, memory byte or stack slot to inspect it. [ and ] jump to the previous or next instruction that changed it; double-click jumps straight to the last writer.",
    "Backspace returns to where you were before a jump; Shift+Backspace goes forward again.",
    "Addresses are shown relative to stack0 (%rsp when recording started), heap, or a symbol, so they stay the same across runs.",
    "The memory address box and the watch list accept expressions: n, -0x8(%rbp), rsp+16, rsp mod 16, printf@plt.",
    "Press p to predict each instruction's effect before you see it.",
  ];

  const keys: [string, string][] = [
    ["→ l / ← h", "next / previous instruction"],
    ["s / S", "step over forwards / backwards"],
    ["n / N", "next / previous source line"],
    ["o / O", "step out forwards / backwards"],
    ["[ / ]", "previous / next change of the selection"],
    ["Backspace / Shift+Backspace", "back / forward in jump history (Alt+← / → work too)"],
    ["g", "focus the locator box"],
    ["m", "bookmark this step"],
    ["p", "toggle predict mode"],
    ["Home / End", "first / last step"],
    ["?", "this help"],
    ["Esc", "close help, else clear the selection"],
  ];

  let dialog: HTMLDivElement;

  onMount(() => {
    dialog.focus();
    if (!lab.prefs.helpSeen) {
      lab.prefs.helpSeen = true;
      lab.savePrefs();
    }
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={(e) => { if (e.target === e.currentTarget) lab.helpOpen = false; }}>
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Help" tabindex="-1" bind:this={dialog}>
    <h1>Reading SystemsLab</h1>
    <ol>
      {#each concepts as c}<li>{c}</li>{/each}
    </ol>
    <table>
      <tbody>
        {#each keys as [k, what]}
          <tr><td>{#each k.split(" ") as part}{#if part === "/"}<span class="dim">/</span>{:else}<kbd>{part}</kbd>{/if}{" "}{/each}</td><td>{what}</td></tr>
        {/each}
      </tbody>
    </table>
    <p class="dim foot">Esc, ? or a click outside closes this.</p>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; background: #0007; }
  .dialog {
    width: min(640px, calc(100vw - 32px)); max-height: calc(100vh - 32px); overflow: auto; outline: none;
    background: var(--panel); border: 1px solid var(--border); border-radius: 8px; padding: 14px 20px 10px;
    box-shadow: 0 8px 32px #0006;
  }
  h1 { margin: 0 0 8px; font-size: 16px; }
  ol { margin: 0 0 12px; padding-left: 20px; }
  li { margin-bottom: 4px; }
  table { border-collapse: collapse; width: 100%; }
  td { padding: 2px 8px 2px 0; vertical-align: top; }
  td:first-child { white-space: nowrap; width: 1%; }
  .foot { margin: 10px 0 0; font-size: 12px; }
</style>
