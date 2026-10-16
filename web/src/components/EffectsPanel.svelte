<script lang="ts">
  import { api, type Effects } from "../lib/api";
  import { lab } from "../lib/state.svelte";
  import { describe } from "../lib/mnemonics";
  import { asmTokens, big, escapeBytes, hex, parsePrediction, predictionMatches, SUBREGS } from "../lib/format";

  type Verdict = { ok: boolean | null; said: string };
  type FlagGuess = "?" | "0" | "1";
  type Branch = Extract<NonNullable<Effects["control"]>, { kind: "branch" }>;

  /** What the learner has guessed for one step. An unrevealed draft is dropped when another step is displayed; a revealed one is kept, so coming back shows the answers again and never scores twice. */
  class Draft {
    reg = $state<Record<string, string>>({});
    mem = $state<Record<string, string>>({});
    flag = $state<Record<string, FlagGuess>>({});
    branch = $state<"" | "taken" | "not">("");
    errors = $state<Record<string, string>>({});
    verdicts = $state<Record<string, Verdict>>({});
    revealed = $state(false);
    busy = $state(false);
    /** Long write lists (a signal frame is hundreds of bytes) show their first entries until asked. */
    all = $state(false);
  }

  /** Full register name for every name an instruction can spell (`eax`, `bl`, … → `rax`, `rbx`). */
  const FULL: Record<string, string> = {};
  for (const [full, subs] of Object.entries(SUBREGS)) for (const s of subs) FULL[s.name] = full;

  const cur = $derived(lab.cur);
  const fx = $derived(cur?.effects ?? null);
  const regNames = $derived(new Set(cur?.regs.map((r) => r.name)));
  const revealedDrafts = new Map<number, Draft>();
  let draftGeneration = lab.status.generation;
  const draft = $derived.by(() => {
    if (!cur) return null;
    if (draftGeneration !== lab.status.generation) {
      revealedDrafts.clear(); // the recording was replaced: old answers no longer describe it
      draftGeneration = lab.status.generation;
    }
    return revealedDrafts.get(cur.step) ?? new Draft();
  });

  const endText = $derived.by(() => {
    const e = lab.info?.end;
    if (!e) return "";
    if (e.reason === "exit") return `exit ${e.code ?? 0}`;
    if (e.reason === "signal") return `signal ${e.signal ?? "?"}`;
    return "step limit reached";
  });

  const regFields = $derived(fx?.reg_writes ?? []);
  const flagFields = $derived(fx?.flags_written.filter((f) => !f.undefined) ?? []);
  const branch = $derived<Branch | null>(fx?.control?.kind === "branch" && fx.control.conditional ? fx.control : null);
  const memFields = $derived(
    (fx?.mem_writes ?? []).flatMap((w, i) => (w.kind === "instr" && w.size <= 8 ? [{ key: String(i), w }] : [])),
  );
  // Faults, signal delivery and the process exit write registers the kernel chooses; there is nothing to predict there.
  const predicting = $derived(lab.prefs.predict && fx?.kind === "instr" && regFields.length + flagFields.length + memFields.length + (branch ? 1 : 0) > 0);

  const LIMIT = 8;
  const regWrites = $derived(draft?.all ? regFields : regFields.slice(0, LIMIT));
  const memWrites = $derived(draft?.all ? (fx?.mem_writes ?? []) : (fx?.mem_writes ?? []).slice(0, LIMIT));
  const hiddenWrites = $derived(regFields.length - regWrites.length + (fx?.mem_writes.length ?? 0) - memWrites.length);
  /** Outcomes stay hidden while a prediction is open. */
  const show = $derived(!predicting || (draft?.revealed ?? true));

  const desc = $derived(fx && fx.kind !== "deliver" ? describe(fx.mnemonic) : undefined);
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
  const withNote = (value: string, note: string | null) => (note ? `${value} (${note})` : value);
  type SyscallEvent = Extract<NonNullable<Effects["event"]>, { kind: "syscall" }>;
  type SignalEvent = Extract<NonNullable<Effects["event"]>, { kind: "signal" }>;
  const signalText = (ev: SignalEvent) => `${ev.signal}${ev.code_name ? ` (${ev.code_name}${ev.code_text ? `: ${ev.code_text}` : ""})` : ""}`;
  const returnText = (ev: SyscallEvent) => (ev.errno ? `${ev.ret}, errno: ${ev.errno}${ev.errno_text ? ` (${ev.errno_text})` : ""}` : (ev.ret ?? ""));
  const deliveredText = (d: NonNullable<Effects["delivered"]>) =>
    `The kernel delivered ${d.signal} and ${d.handler ? `entered the handler at ${d.handler}` : "the process ended"}.`;

  function fullReg(name: string): string | null {
    const full = FULL[name] ?? name;
    return regNames.has(full) ? full : null;
  }

  const regLabel = (name: string) => (name.endsWith("_base") ? name : `%${name}`);

  function showAddr(raw: string, size: number) {
    lab.showInMemory(raw);
    lab.select({ kind: "mem", addr: raw, size });
  }

  /** The address behind a rendered `stack0-0x30`-style text that one of this step's memory accesses used. */
  function memOf(rendered: string): { raw: string; size: number } | null {
    const m = [...(fx?.mem_writes ?? []), ...(fx?.mem_reads ?? [])].find((x) => x.addr === rendered);
    return m ? { raw: m.raw_addr, size: m.size } : null;
  }

  function cycle(d: Draft, name: string) {
    const next: Record<FlagGuess, FlagGuess> = { "?": "0", "0": "1", "1": "?" };
    d.flag[name] = next[d.flag[name] ?? "?"];
  }

  function togglePredict() {
    lab.prefs.predict = !lab.prefs.predict;
    lab.savePrefs();
  }

  async function reveal() {
    const d = draft;
    const c = cur;
    const e = fx;
    if (!d || !c || !e || d.busy || d.revealed) return;

    // Validate everything first so a typo never costs a request.
    type Guess = { key: string; said: string; pred: ReturnType<typeof parsePrediction> & { ok: true }; bits: number; raw?: () => Promise<string | null>; after?: string };
    const guesses: Guess[] = [];
    const errors: Record<string, string> = {};
    const note = (key: string, text: string, bits: number, extra: Pick<Guess, "raw" | "after">) => {
      const said = text.trim();
      if (said === "") return;
      const pred = parsePrediction(said);
      if (pred.ok) guesses.push({ key, said, pred, bits, ...extra });
      else errors[key] = pred.error;
    };
    for (const f of regFields) note(`r:${f.name}`, d.reg[f.name] ?? "", f.name.startsWith("xmm") ? 128 : 64, { after: f.after });
    for (const { key, w } of memFields) {
      note(`m:${key}`, d.mem[key] ?? "", w.size * 8, {
        raw: async () => {
          const m = await api.mem(w.raw_addr, w.size, c.step + 1);
          if (m.bytes.length < w.size || m.bytes.some((b) => b === null)) return null;
          return hex(m.bytes.reduce<bigint>((v, b, i) => v | (BigInt(b ?? 0) << BigInt(8 * i)), 0n));
        },
      });
    }
    if (Object.keys(errors).length) {
      d.errors = errors;
      return;
    }

    d.busy = true;
    d.errors = {};
    const verdicts: Record<string, Verdict> = {};
    let answered = 0;
    let right = 0;
    const grade = (key: string, said: string, ok: boolean | null) => {
      verdicts[key] = { ok, said };
      if (ok !== null) {
        answered += 1;
        if (ok) right += 1;
      }
    };
    try {
      await Promise.all(
        guesses.map(async (g) => {
          try {
            const pred = "value" in g.pred ? g.pred.value : big((await api.eval(g.pred.expr, c.step)).value);
            const recorded = g.after ?? (await g.raw?.()) ?? null;
            grade(g.key, g.said, recorded === null ? null : predictionMatches(pred, recorded, g.bits));
          } catch (err) {
            errors[g.key] = err instanceof Error ? err.message : String(err);
          }
        }),
      );
    } finally {
      if (draft === d) d.busy = false;
    }
    if (draft !== d) return; // the learner moved on while the answers were being checked
    if (Object.keys(errors).length) {
      d.errors = errors;
      return;
    }
    for (const f of flagFields) {
      const g = d.flag[f.name] ?? "?";
      if (g !== "?") grade(`f:${f.name}`, g, Number(g) === f.after);
    }
    if (branch && d.branch !== "") grade("b", d.branch === "taken" ? "taken" : "not taken", (d.branch === "taken") === branch.taken);
    d.verdicts = verdicts;
    d.revealed = true;
    revealedDrafts.set(c.step, d);
    lab.predictScore.total += answered;
    lab.predictScore.right += right;
  }

  /** Enter reveals while nothing that handles Enter itself has focus. */
  function onkey(e: KeyboardEvent) {
    if (e.key !== "Enter" || e.ctrlKey || e.metaKey || e.altKey || !predicting || lab.helpOpen) return;
    if (e.target instanceof Element && e.target.closest("input, textarea, select, button, a")) return;
    e.preventDefault();
    void reveal();
  }

  function onInputKey(e: KeyboardEvent) {
    if (e.key !== "Enter") return;
    e.preventDefault();
    void reveal();
  }
</script>

<svelte:window onkeydown={onkey} />

{#snippet reg(name: string)}
  {@const full = fullReg(name)}
  {#if full}
    <button class="lnk reg" title="Select {full}" onclick={() => lab.select({ kind: "reg", name: full })}>{regLabel(name)}</button>
  {:else}
    <span class="reg">{regLabel(name)}</span>
  {/if}
{/snippet}

{#snippet addr(raw: string, rendered: string, size: number)}
  <button class="lnk addr" title="Show {raw} in the Memory tab" onclick={() => showAddr(raw, size)}>{rendered}</button>
{/snippet}

{#snippet stackSlot(rendered: string)}
  {@const m = memOf(rendered)}
  {#if m}{@render addr(m.raw, rendered, m.size)}{:else}<span>{rendered}</span>{/if}
{/snippet}

{#snippet verdict(key: string)}
  {@const v = draft?.verdicts[key]}
  {#if v}
    {#if v.ok === true}
      <span class="ok" title="You said {v.said}">✓</span>
    {:else if v.ok === false}
      <span class="bad">✗ you said {v.said}</span>
    {:else}
      <span class="dim">(could not check)</span>
    {/if}
  {/if}
{/snippet}

{#snippet value(raw: string, rendered: string)}
  <span>{raw}</span>{#if rendered !== raw}{" "}<span class="dim">({rendered})</span>{/if}
{/snippet}

{#snippet syscall(ev: Extract<NonNullable<Effects["event"]>, { kind: "syscall" }>)}
  <div class="grp">
    <span class="lbl">Syscall</span>
    <div class="items">
      <div class="mono">{ev.name}({ev.args.map((a) => `${a.name}=${withNote(a.value, a.note)}`).join(", ")})</div>
      <div class="dim">
        number in {@render reg("rax")} = {ev.nr}; {#if ev.args.length}arguments in {#each ev.args as a, i}{#if i}{", "}{/if}{@render reg(a.reg)}{/each}{:else}no arguments{/if}
      </div>
      {#if show}
        <div>
          {#if ev.ret === null}
            does not return
          {:else}
            returns <span class="mono">{returnText(ev)}</span>
          {/if}
        </div>
      {/if}
      {#each ev.buffers.filter((b) => show || b.dir === "in") as b}
        <div>
          <span class="mono">{b.arg}</span>
          <span class="dim">({plural(b.len, "byte")}, {b.dir === "in" ? "read by the kernel" : "written by the kernel"})</span>
          {#if b.text === null}<span class="dim">binary data</span>{:else}<span class="mono">"{escapeBytes(b.text)}"{b.truncated ? "…" : ""}</span>{/if}
        </div>
      {/each}
      <div class="dim">The syscall instruction also overwrites %rcx and %r11.</div>
    </div>
  </div>
{/snippet}

<div class="panel">
  <h2 class="head">
    <span class="title">Effects{cur && fx ? ` · step ${cur.step} → ${cur.step + 1}` : cur ? ` · step ${cur.step}` : ""}</span>
    {#if lab.predictScore.total > 0}
      <span class="score" title="Your predictions that were exactly right, out of all you answered">✓ {lab.predictScore.right} / {lab.predictScore.total}</span>
    {/if}
    <label class="toggle" title="Guess each result before it is shown (key p)">
      <input type="checkbox" checked={lab.prefs.predict} onchange={togglePredict} /> predict
    </label>
  </h2>
  <div class="body">
    {#if !cur}
      <div class="dim pad">loading…</div>
    {:else if !fx}
      <div class="pad">The recording ends here: {endText}.</div>
    {:else}
      <div class="insn mono">{#each asmTokens(cur.asm) as t}<span class={t.cls}>{t.text}</span>{/each}</div>
      {#if desc}<p class="desc dim">{desc}</p>{/if}

      {#if fx.reg_reads.length || fx.mem_reads.length || (!fx.condition && fx.flags_read.length)}
        <div class="grp">
          <span class="lbl">Reads</span>
          <div class="items mono">
            {#each fx.reg_reads as r (r.name)}
              <div>{@render reg(r.name)} = {@render value(r.value, r.rendered)}</div>
            {/each}
            {#each fx.mem_reads as m}
              <div>{@render addr(m.raw_addr, m.addr, m.size)} <span class="dim">({plural(m.size, "byte")})</span> = {m.value}</div>
            {/each}
            {#if !fx.condition && fx.flags_read.length}
              <div>{#each fx.flags_read as f, i}{#if i}{", "}{/if}{f.name} = {f.value}{/each}</div>
            {/if}
          </div>
        </div>
      {/if}

      {#if show}
        {#if fx.reg_writes.length || fx.mem_writes.length}
          <div class="grp">
            <span class="lbl">Writes</span>
            <div class="items mono">
              {#each regWrites as w (w.name)}
                <div>
                  {@render reg(w.name)}{#if w.via}{" "}<span class="dim">(via %{w.via})</span>{/if}: <span title={w.before_rendered}>{w.before}</span> → {@render value(w.after, w.after_rendered)}
                  {#if !w.changed}<span class="dim">unchanged</span>{/if}
                  {@render verdict(`r:${w.name}`)}
                </div>
              {/each}
              {#each memWrites as w, i}
                <div>
                  {@render addr(w.raw_addr, w.addr, w.size)} <span class="dim">({plural(w.size, "byte")}{w.kind === "instr" ? "" : `, ${w.kind}`})</span>: {w.before} → {w.after}
                  {@render verdict(`m:${i}`)}
                </div>
              {/each}
              {#if hiddenWrites > 0}
                <button class="lnk more" onclick={() => draft && (draft.all = true)}>show {hiddenWrites} more write{hiddenWrites === 1 ? "" : "s"}</button>
              {/if}
            </div>
          </div>
        {/if}
        {#if fx.flags_written.length}
          <div class="grp">
            <span class="lbl">Flags</span>
            <div class="items mono flags">
              {#each fx.flags_written as f (f.name)}
                <span class="flag" class:chg={!f.undefined && f.before !== f.after} class:und={f.undefined} title={f.undefined ? `${f.name} is undefined after this instruction` : ""}>
                  {f.name} {f.undefined ? "?" : `${f.before}→${f.after}`}{@render verdict(`f:${f.name}`)}
                </span>
              {/each}
            </div>
          </div>
        {/if}
      {:else if draft}
        <div class="grp predict">
          <span class="lbl">Predict</span>
          <div class="items">
            <div class="dim">What will this instruction write? Type a number (42, -1, 0x2a) or stack0/heap ± offset; blank fields are skipped.</div>
            {#each regFields as f (f.name)}
              <div class="field">
                <label for="p-r-{f.name}" class="fl mono">{regLabel(f.name)}{#if f.via}{" "}<span class="dim">(written via %{f.via})</span>{/if}</label>
                <input id="p-r-{f.name}" class="mono" spellcheck="false" autocomplete="off" placeholder="value after" value={draft.reg[f.name] ?? ""} oninput={(e) => (draft.reg[f.name] = e.currentTarget.value)} onkeydown={onInputKey} />
              </div>
              {#if draft.errors[`r:${f.name}`]}<div class="err">{draft.errors[`r:${f.name}`]}</div>{/if}
            {/each}
            {#each memFields as { key, w }, n (key)}
              <div class="field">
                <label for="p-m-{key}" class="fl mono">memory {memFields.length > 1 ? `${n + 1} ` : ""}<span class="dim">({plural(w.size, "byte")})</span></label>
                <input id="p-m-{key}" class="mono" spellcheck="false" autocomplete="off" placeholder="value stored" value={draft.mem[key] ?? ""} oninput={(e) => (draft.mem[key] = e.currentTarget.value)} onkeydown={onInputKey} />
              </div>
              {#if draft.errors[`m:${key}`]}<div class="err">{draft.errors[`m:${key}`]}</div>{/if}
            {/each}
            {#if flagFields.length}
              <div class="field">
                <span class="fl">flags</span>
                <span class="flagpick">
                  {#each flagFields as f (f.name)}
                    <button class="tog mono" class:on={(draft.flag[f.name] ?? "?") !== "?"} title="{f.name} after the instruction: click to cycle ? → 0 → 1" onclick={() => cycle(draft, f.name)}>
                      {f.name} {draft.flag[f.name] ?? "?"}
                    </button>
                  {/each}
                </span>
              </div>
            {/if}
            {#if branch}
              <div class="field">
                <span class="fl">branch</span>
                <span class="flagpick">
                  <button class="tog" class:on={draft.branch === "taken"} onclick={() => (draft.branch = draft.branch === "taken" ? "" : "taken")}>taken</button>
                  <button class="tog" class:on={draft.branch === "not"} onclick={() => (draft.branch = draft.branch === "not" ? "" : "not")}>not taken</button>
                </span>
              </div>
            {/if}
            <div class="field">
              <button class="reveal" disabled={draft.busy} onclick={reveal}>Reveal</button>
              <span class="dim"><kbd>Enter</kbd></span>
            </div>
          </div>
        </div>
      {/if}

      {#if fx.condition}
        {@const c = fx.condition}
        <div class="grp">
          <span class="lbl">Condition</span>
          <div class="items">
            <div class="mono"><b>{c.code}</b>: {c.text}{#if show}{" → "}<span class={c.holds ? "ok" : "no"}>{c.holds ? "holds" : "does not hold"}</span>{" "}<span class="dim">({c.meaning})</span>{/if}</div>
            {#if fx.flags_read.length}
              <div class="dim mono">{#each fx.flags_read as f, i}{#if i}{", "}{/if}{f.name} = {f.value}{/each}</div>
            {/if}
          </div>
        </div>
      {/if}

      {#if fx.control}
        {@const ctl = fx.control}
        {#if show || ctl.kind === "call"}
          <div class="grp">
            <span class="lbl">Control</span>
            <div class="items mono">
              {#if ctl.kind === "branch" && show}
                <div>
                  {#if ctl.conditional}{ctl.taken ? "taken" : "not taken"} → {ctl.taken ? ctl.target : ctl.fallthrough}{:else}jumps to {ctl.target}{/if}
                  {@render verdict("b")}
                </div>
              {:else if ctl.kind === "call"}
                {#if show}
                  <div>pushes return address {ctl.return_address} at {@render stackSlot(ctl.slot)}, %rsp {ctl.rsp_before} → {ctl.rsp_after}, jumps to {ctl.target}</div>
                {/if}
                <div>
                  %rsp mod 16 at the call: {ctl.rsp_mod16}
                  {#if ctl.rsp_mod16 === 0}<span class="ok">✓</span>{:else}<span class="bad">✗</span> <span class="dim">(System V requires 0)</span>{/if}
                </div>
              {:else if ctl.kind === "ret" && show}
                <div>pops {@render stackSlot(ctl.slot)} = {ctl.to}, %rsp {ctl.rsp_before} → {ctl.rsp_after}</div>
                {#if ctl.call_step !== null}
                  {@const k = ctl.call_step}
                  <div class="dim">closes the call at <button class="lnk" onclick={() => lab.goto(k, { record: true })}>step {k}</button></div>
                {/if}
              {/if}
            </div>
          </div>
        {/if}
      {/if}

      {#if fx.kind === "deliver" && fx.delivered}
        <div class="line">{deliveredText(fx.delivered)}</div>
      {/if}
      {#if fx.kind === "exit"}<div class="line">The process exited during this step.</div>{/if}
      {#if fx.event?.kind === "syscall"}
        {@render syscall(fx.event)}
      {:else if fx.event?.kind === "signal"}
        {@const ev = fx.event}
        <div class="line">
          {#if fx.kind === "fault"}
            Raised {signalText(ev)}{ev.cpu_exception ? `, CPU exception ${ev.cpu_exception}` : ""}. The instruction did not complete; the next step delivers the signal.
          {:else}
            Signal {signalText(ev)}.
          {/if}
        </div>
      {/if}

      {#if show && fx.kind !== "exit" && !(fx.delivered && !fx.delivered.handler)}
        <div class="next dim">Next: <span class="mono">{fx.next_loc}</span></div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .panel > h2.head { display: flex; align-items: center; gap: 10px; overflow: hidden; }
  .title { flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .score, .toggle { flex: none; text-transform: none; letter-spacing: 0; font-weight: 400; font-size: 12px; }
  .score { color: var(--c1); }
  .toggle { display: flex; align-items: center; gap: 4px; cursor: pointer; }
  .toggle input { margin: 0; padding: 0; }

  .body { font-size: 12px; padding: 4px 8px 6px; min-height: 5.5rem; }
  .pad { padding: 4px 0; }
  .insn { font-size: 13px; font-weight: 600; white-space: pre-wrap; }
  .desc { margin: 2px 0 4px; line-height: 1.4; }
  .grp { display: grid; grid-template-columns: 5.2em minmax(0, 1fr); gap: 8px; padding: 2px 0; }
  .lbl { color: var(--dim); font-size: 11px; padding-top: 2px; }
  .items { min-width: 0; line-height: 1.5; overflow-wrap: anywhere; }
  .line { padding: 2px 0; line-height: 1.5; }
  .next { padding-top: 3px; }

  .reg { color: var(--c3); }
  .imm { color: var(--orange); }
  .sym { color: var(--c2); }
  .lnk { background: none; border: none; padding: 0; font: inherit; color: var(--c3); cursor: pointer; text-align: left; overflow-wrap: anywhere; }
  .lnk.addr { color: inherit; text-decoration: underline dotted var(--dim); text-underline-offset: 2px; }
  .lnk:hover:not(:disabled) { text-decoration: underline; }
  .lnk.more { display: block; color: var(--accent); font-family: var(--font); }
  .ok { color: var(--c1); font-weight: 600; }
  .no { color: var(--orange); font-weight: 600; }
  .bad { color: var(--red); font-weight: 600; }

  .flags { display: flex; flex-wrap: wrap; gap: 2px 12px; }
  .flag { color: var(--dim); }
  .flag.chg { color: var(--fg); font-weight: 700; }
  .flag.und { font-style: italic; }

  .predict .items { display: flex; flex-direction: column; gap: 3px; }
  .field { display: flex; align-items: center; gap: 8px; }
  .fl { flex: none; min-width: 15ch; }
  .field input { flex: 1 1 auto; max-width: 28ch; min-width: 0; font-size: 12px; padding: 1px 6px; }
  .flagpick { display: flex; flex-wrap: wrap; gap: 4px; }
  .tog { font-size: 12px; padding: 0 6px; }
  .tog.on { border-color: var(--accent); background: var(--cur); }
  .reveal { padding: 1px 12px; border-color: var(--accent); }
  .err { color: var(--red); padding-left: calc(15ch + 8px); }
</style>
