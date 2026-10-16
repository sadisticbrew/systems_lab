<script lang="ts">
  import { stepOf } from "../lib/bookmarks.svelte";
  import { lab } from "../lib/state.svelte";

  const bookmarks = $derived(lab.notes.bookmarks ?? []);

  function remove(id: string) {
    lab.notes.bookmarks = bookmarks.filter((b) => b.id !== id);
    lab.saveNotes();
  }
</script>

<div class="notes">
  <div class="cols">
    <section class="marks">
      <div class="bar">
        <button onclick={() => lab.addBookmark()} disabled={!lab.cur}>Bookmark this step (m)</button>
        {#if !bookmarks.length}<span class="dim">Bookmarks keep a moment you want to come back to.</span>{/if}
      </div>
      {#each bookmarks as b (b.id)}
        {@const step = stepOf(b)}
        <div class="bm">
          <div class="line">
            <input class="label" aria-label="Bookmark label" bind:value={b.label} oninput={() => lab.saveNotes()} />
            <button disabled={step === null} onclick={() => lab.locate(b.locator)}>Go</button>
            <button onclick={() => remove(b.id)}>Delete</button>
          </div>
          <div class="meta mono">
            <span>{b.locator}</span>
            {#if step === null}<span class="gone">not found in this recording</span>{:else}<span class="dim">step {step}</span>{/if}
          </div>
          <textarea class="note" rows="2" placeholder="Note" aria-label="Bookmark note" bind:value={b.note} oninput={() => lab.saveNotes()}></textarea>
        </div>
      {/each}
    </section>
    <section class="trace">
      <label for="trace-notes" class="dim">Notes on this trace</label>
      <textarea id="trace-notes" placeholder="Anything you want to remember about this recording" bind:value={lab.notes.notes} oninput={() => lab.saveNotes()}></textarea>
    </section>
  </div>
  <div class="foot dim">Saved to {lab.info?.notes_path ?? "…"}</div>
</div>

<style>
  .notes { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .cols { flex: 1; min-height: 0; display: flex; gap: 12px; padding: 6px 10px 0; }
  section { min-width: 0; min-height: 0; display: flex; flex-direction: column; gap: 4px; }
  .marks { flex: 3 1 0; overflow: auto; }
  .trace { flex: 2 1 0; }
  .trace textarea { flex: 1; min-height: 40px; resize: none; }
  .bar { display: flex; align-items: center; gap: 10px; font-size: 12px; }
  .bm { display: flex; flex-direction: column; gap: 2px; padding: 4px 0 6px; border-bottom: 1px solid var(--border); }
  .line { display: flex; gap: 4px; }
  .label { flex: 1; min-width: 0; }
  .meta { display: flex; gap: 12px; font-size: 11px; }
  .gone { color: var(--red); }
  .note { resize: vertical; font-size: 12px; }
  .foot { flex: none; padding: 2px 10px 3px; font-size: 11px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
