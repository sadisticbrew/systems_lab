<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { api, type WatchType } from "../lib/api";
  import { big, parsePrediction, predictionMatches } from "../lib/format";
  import { drafts, type Predict } from "../lib/guide";
  import { lab } from "../lib/state.svelte";
  import GuideText from "./GuideText.svelte";

  let { p }: { p: Predict } = $props();

  /** Types whose recorded value is text; the learner's answer is compared with it as typed. */
  const AS_TEXT: WatchType[] = ["f32", "f64", "str", "bytes"];
  /** Width the answer is compared at; every other type compares 64 bits. */
  const BITS: Partial<Record<WatchType, number>> = { u8: 8, i8: 8, u16: 16, i16: 16, u32: 32, i32: 32 };

  type Outcome = { correct: boolean; step: number; value: string; raw: string | null } | { error: string };

  const stored = $derived(lab.notes.guide?.[p.key]);
  let answer = $state(untrack(() => lab.notes.guide?.[p.key]?.answer ?? drafts.get(p.key) ?? ""));
  let result = $state.raw<Outcome | null>(null);
  let busy = $state(false);
  let peek = $state(false);

  const verdict = $derived(result && "correct" in result ? result : null);
  const shown = $derived(verdict ? verdict.correct : stored && !result ? stored.correct : null);

  /** Compares `text` with what the recording holds for this directive; touches neither the notes nor the position. */
  async function evaluate(text: string): Promise<Outcome> {
    try {
      const step = (await api.locate(p.loc)).step + (p.after ? 1 : 0);
      if (step > lab.last) return { error: "That moment is past the end of the recording." };
      const w = (await api.watch(step, [{ expr: p.expr, type: p.type ?? undefined }])).watches[0];
      if (!w || w.error !== null) return { error: w?.error ?? "The recording has no value for this." };
      if (w.value === null) return { error: "That value cannot be read at this moment of the recording." };
      if (AS_TEXT.includes(w.type)) return { correct: text === w.value, step, value: w.value, raw: w.raw };
      const guess = parsePrediction(text);
      if (!guess.ok) return { error: guess.error };
      if (w.raw === null) return { error: "That value cannot be read at this moment of the recording." };
      const n = "value" in guess ? guess.value : big((await api.eval(guess.expr, step)).value);
      return { correct: predictionMatches(n, w.raw, BITS[w.type] ?? 64), step, value: w.value, raw: w.raw };
    } catch (e) {
      return { error: e instanceof Error ? e.message : String(e) };
    }
  }

  function remember(text: string, correct: boolean) {
    lab.notes.guide = { ...lab.notes.guide, [p.key]: { answer: text, correct } };
    lab.saveNotes();
  }

  async function check() {
    const text = answer.trim();
    if (!text) {
      result = { error: "Type your prediction first." };
      return;
    }
    busy = true;
    const out = await evaluate(text);
    busy = false;
    result = out;
    if ("error" in out) return;
    drafts.delete(p.key);
    remember(text, out.correct);
    void lab.goto(out.step, { record: true });
  }

  /** A stored answer is checked again against the recording that is loaded now; the position stays. */
  onMount(async () => {
    const s = stored;
    if (!s) return;
    const out = await evaluate(s.answer);
    result = out;
    if (!("error" in out) && out.correct !== s.correct) remember(s.answer, out.correct);
  });
</script>

<div class="card">
  <div class="q">
    Predict <code>{p.expr}</code>{p.type ? ` as ${p.type}` : ""} at <code>{p.loc}</code>{p.after ? " (after it runs)" : ""}
  </div>
  <div class="ask">
    <input
      class="mono"
      placeholder="your answer"
      aria-label="Your prediction"
      bind:value={answer}
      oninput={() => drafts.set(p.key, answer)}
      onkeydown={(e) => e.key === "Enter" && check()}
    />
    <button onclick={check} disabled={busy}>Check</button>
    {#if shown !== null}<span class="mark" class:right={shown} class:wrong={!shown} title={shown ? "Your answer matched the recording" : "Your answer did not match the recording"}>{shown ? "✓" : "✗"}</span>{/if}
    {#if verdict}
      <span class="mono">recorded: <b>{verdict.value}</b>{verdict.raw !== null && verdict.raw !== verdict.value ? ` (${verdict.raw})` : ""}</span>
    {/if}
  </div>
  {#if result && "error" in result}<div class="err">{result.error}</div>{/if}
  {#if p.explanation.length}
    {#if stored || peek}
      <div class="why">
        {#each p.explanation as para}<p><GuideText spans={para} /></p>{/each}
      </div>
    {:else}
      <button class="link" onclick={() => (peek = true)}>Show explanation</button>
    {/if}
  {/if}
</div>

<style>
  .card { margin: 8px 0; padding: 6px 10px 8px; border: 1px solid var(--border); border-left: 3px solid var(--accent); border-radius: 4px; background: var(--bg); }
  .q { margin-bottom: 4px; }
  .ask { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  input { width: 18ch; }
  .mark { font-weight: 700; font-size: 15px; }
  .right { color: var(--c1); }
  .wrong { color: var(--red); }
  .err { margin-top: 4px; color: var(--red); }
  .why { margin-top: 6px; }
  .why p { margin: 4px 0; }
  .link { margin-top: 6px; border: none; background: none; padding: 0; color: var(--accent); }
  .link:hover:not(:disabled) { text-decoration: underline; }
  code { font-family: var(--mono); background: var(--panel); border: 1px solid var(--border); border-radius: 3px; padding: 0 3px; }
</style>
