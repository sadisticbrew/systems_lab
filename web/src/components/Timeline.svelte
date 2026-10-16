<script lang="ts">
  import { untrack } from "svelte";
  import { api, type Marker, type Timeline } from "../lib/api";
  import { lab } from "../lib/state.svelte";
  import { resolveBookmarks, stepOf } from "../lib/bookmarks.svelte";

  const KINDS = ["call", "ret", "syscall", "signal", "abi", "bookmark"] as const;
  type Kind = (typeof KINDS)[number];
  type Mark = { step: number; kind: Kind; text: string };

  const COLOR: Record<Kind, string> = { call: "--c1", ret: "--c2", syscall: "--c3", signal: "--red", abi: "--orange", bookmark: "--accent" };
  const HINT: Record<Kind, string> = {
    call: "a function is called",
    ret: "a function returns",
    syscall: "the program asks the kernel for something",
    signal: "a signal arrives",
    abi: "a System V rule is broken",
    bookmark: "a moment you bookmarked",
  };
  const MIN_SPAN = 50;
  const WHEEL_NOTCH = 40;

  let plot: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let width = $state(0);
  let height = $state(0);
  let dragging = false;
  let theme = $state(0);

  /** Zoomed window of states (inclusive), or `null` for the whole recording. */
  let view = $state<{ from: number; to: number } | null>(null);
  /** The timeline fetched for `view`; until it arrives the whole-recording data is stretched over the window. */
  let zoomed = $state.raw<Timeline | null>(null);
  let hover = $state<{ cx: number; cy: number; step: number; depth: string; mark: Mark | null } | null>(null);
  let fetchTimer: number | undefined;
  let fetchId = 0;
  let wheelAcc = 0;

  const win = $derived(view ?? { from: 0, to: lab.last });
  const data = $derived(view && zoomed ? zoomed : lab.timeline);
  const maxDepth = $derived(Math.max(1, ...(lab.timeline?.depth ?? [])));

  const on = (k: Kind) => lab.prefs.markerFilter[k] !== false;

  function markerText(m: Marker): string {
    return m.kind === "ret" ? m.label : `${m.kind} ${m.label}`;
  }

  const counts = $derived.by(() => {
    const c: Record<Kind, number> = { call: 0, ret: 0, syscall: 0, signal: 0, abi: 0, bookmark: 0 };
    for (const m of lab.timeline?.markers ?? []) c[m.kind]++;
    for (const b of lab.notes.bookmarks ?? []) if (stepOf(b) !== null) c.bookmark++;
    return c;
  });

  /** The marks inside the window whose kind is switched on. */
  const marks = $derived.by(() => {
    const out: Mark[] = [];
    for (const m of data?.markers ?? []) {
      if (on(m.kind) && m.step >= win.from && m.step <= win.to) out.push({ step: m.step, kind: m.kind, text: markerText(m) });
    }
    if (on("bookmark")) {
      for (const b of lab.notes.bookmarks ?? []) {
        const s = stepOf(b);
        if (s !== null && s >= win.from && s <= win.to) out.push({ step: s, kind: "bookmark", text: `bookmark: ${b.label}` });
      }
    }
    return out;
  });

  const count = $derived(win.to - win.from + 1);
  /** Horizontal centre of state `s`'s cell. */
  const px = (s: number) => ((s - win.from + 0.5) / count) * width;
  const stepAt = (x: number) => Math.min(win.to, Math.max(win.from, win.from + Math.floor((x / Math.max(1, width)) * count)));

  $effect(() => {
    const ro = new ResizeObserver(() => {
      width = plot.clientWidth;
      height = plot.clientHeight;
    });
    ro.observe(plot);
    return () => ro.disconnect();
  });

  $effect(() => {
    const mq = matchMedia("(prefers-color-scheme: dark)");
    const bump = () => theme++;
    mq.addEventListener("change", bump);
    return () => mq.removeEventListener("change", bump);
  });

  // The wheel listener must be non-passive to keep the page from scrolling under the pointer.
  $effect(() => {
    canvas.addEventListener("wheel", onwheel, { passive: false });
    return () => canvas.removeEventListener("wheel", onwheel);
  });

  $effect(() => {
    void lab.status.generation;
    untrack(resetZoom);
  });

  $effect(() => {
    void lab.status.generation;
    void lab.notes.bookmarks?.map((b) => b.locator);
    void resolveBookmarks();
  });

  const css = (n: string) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();

  function draw() {
    void theme;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.max(1, Math.floor(width * dpr));
    canvas.height = Math.max(1, Math.floor(height * dpr));
    const g = canvas.getContext("2d")!;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, width, height);
    if (!data || width === 0) return;

    const span = win.to - win.from + 1;
    g.fillStyle = css("--sil");
    const buckets = data.depth.length;
    const dataCount = data.to - data.from + 1;
    for (let i = 0; i < buckets; i++) {
      const lo = data.from + Math.floor((i * dataCount) / buckets);
      const hi = data.from + Math.floor(((i + 1) * dataCount) / buckets);
      const x0 = ((lo - win.from) / span) * width;
      const x1 = ((hi - win.from) / span) * width;
      if (x1 < 0 || x0 > width) continue;
      const h = (data.depth[i] / maxDepth) * (height - 8);
      g.fillRect(x0, height - h, Math.max(1, x1 - x0 + 0.5), h);
    }

    for (const m of marks) {
      const x = Math.floor(px(m.step));
      g.fillStyle = css(COLOR[m.kind]);
      if (m.kind === "bookmark") {
        g.beginPath();
        g.moveTo(x - 4, 0);
        g.lineTo(x + 5, 0);
        g.lineTo(x + 0.5, 7);
        g.fill();
      } else if (m.kind === "call" || m.kind === "ret") {
        g.fillRect(x, height - 10, 1, 10);
      } else {
        g.fillRect(x, 0, 1, height);
      }
    }

    g.fillStyle = css("--fg");
    g.fillRect(Math.round(px(lab.step)) - 1, 0, 2, height);
  }

  $effect(() => {
    void [data, marks, win, lab.step, width, height, maxDepth];
    if (canvas) draw();
  });

  // --- zoom ---
  function resetZoom() {
    clearTimeout(fetchTimer);
    fetchId++;
    view = null;
    zoomed = null;
  }

  function fetchZoomed() {
    clearTimeout(fetchTimer);
    fetchTimer = window.setTimeout(async () => {
      const w = view;
      if (!w) return;
      const id = ++fetchId;
      try {
        const t = await api.timeline(w.from, w.to);
        if (id === fetchId) zoomed = t;
      } catch (e) {
        lab.fail(e);
      }
    }, 120);
  }

  function zoomBy(factor: number, x: number) {
    const last = lab.last;
    const span = win.to - win.from;
    const next = Math.min(last, Math.max(Math.min(MIN_SPAN, last), Math.round(span * factor)));
    if (next === span) return;
    const frac = Math.min(1, Math.max(0, x / Math.max(1, width)));
    const from = Math.min(last - next, Math.max(0, Math.round(win.from + frac * (span + 1) - frac * (next + 1))));
    if (from === 0 && from + next === last) {
      resetZoom();
      return;
    }
    view = { from, to: from + next };
    fetchZoomed();
  }

  function onwheel(e: WheelEvent) {
    if (e.ctrlKey || e.metaKey) return;
    e.preventDefault();
    wheelAcc += e.deltaMode === 1 ? e.deltaY * 16 : e.deltaMode === 2 ? e.deltaY * 400 : e.deltaY;
    if (Math.abs(wheelAcc) < WHEEL_NOTCH) return;
    const zoomIn = wheelAcc < 0;
    wheelAcc = 0;
    zoomBy(zoomIn ? 1 / 1.5 : 1.5, e.clientX - canvas.getBoundingClientRect().left);
  }

  // --- pointer ---
  const xOf = (e: PointerEvent) => Math.max(0, Math.min(width, e.clientX - canvas.getBoundingClientRect().left));

  /** The mark within 3px of `x`, nearest first. */
  function markNear(x: number): Mark | null {
    let best: Mark | null = null;
    let bestDist = 3.0001;
    for (const m of marks) {
      const d = Math.abs(px(m.step) - x);
      if (d < bestDist) {
        best = m;
        bestDist = d;
      }
    }
    return best;
  }

  function depthAt(step: number): string {
    const t = data;
    if (!t || t.depth.length === 0) return "?";
    const dataCount = t.to - t.from + 1;
    const i = Math.min(t.depth.length - 1, Math.max(0, Math.floor(((step - t.from) * t.depth.length) / dataCount)));
    return dataCount > t.depth.length ? `≤ ${t.depth[i]}` : String(t.depth[i]);
  }

  function onmove(e: PointerEvent) {
    const x = xOf(e);
    const step = stepAt(x);
    hover = { cx: e.clientX, cy: e.clientY, step, depth: depthAt(step), mark: markNear(x) };
    if (dragging) lab.goto(step);
  }

  function ondown(e: PointerEvent) {
    dragging = true;
    canvas.setPointerCapture(e.pointerId);
    const x = xOf(e);
    // A click on a mark lands on the moment it names; dragging then scrubs freely.
    lab.goto(markNear(x)?.step ?? stepAt(x), { record: true });
  }

  function toggle(k: Kind) {
    lab.prefs.markerFilter[k] = !on(k);
    lab.savePrefs();
  }
</script>

<div class="tl panel">
  <div class="legend">
    {#each KINDS as k}
      <button class="chip" class:off={!on(k)} aria-pressed={on(k)} title="{k}: {HINT[k]} (click to {on(k) ? 'hide' : 'show'})" onclick={() => toggle(k)}>
        <i style:background="var({COLOR[k]})"></i>{k} <span class="n">{counts[k]}</span>
      </button>
    {/each}
    {#if view}
      <button class="chip reset" title="Show the whole recording again" onclick={resetZoom}>{view.from}–{view.to} ⟲</button>
    {:else}
      <span class="hint dim">scroll to zoom</span>
    {/if}
  </div>
  <div class="plot" bind:this={plot}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <canvas
      bind:this={canvas}
      style="width:{width}px;height:{height}px"
      onpointerdown={ondown}
      onpointermove={onmove}
      onpointerup={() => (dragging = false)}
      onpointercancel={() => (dragging = false)}
      onpointerleave={() => (hover = null)}
    ></canvas>
  </div>
</div>

{#if hover}
  <div class="tip" class:flip={hover.cx > window.innerWidth - 260} style:left="{hover.cx}px" style:top="{hover.cy + 16}px">
    <div>step {hover.step} · depth {hover.depth}</div>
    {#if hover.mark}<div class="mono" style:color="var({COLOR[hover.mark.kind]})">{hover.mark.text}{hover.mark.step !== hover.step ? ` (step ${hover.mark.step})` : ""}</div>{/if}
  </div>
{/if}

<style>
  .legend { display: flex; align-items: center; gap: 4px; height: 18px; flex: none; padding: 0 4px; border-bottom: 1px solid var(--border); font-size: 11px; overflow: hidden; }
  .chip { display: inline-flex; align-items: center; gap: 4px; height: 15px; padding: 0 6px; border-radius: 8px; font-size: 11px; line-height: 1; white-space: nowrap; }
  .chip i { width: 7px; height: 7px; border-radius: 2px; display: inline-block; }
  .chip .n { color: var(--dim); }
  .chip.off { opacity: 0.45; text-decoration: line-through; }
  .chip.reset { margin-left: auto; border-color: var(--accent); color: var(--accent); }
  .hint { margin-left: auto; }
  .plot { flex: 1; min-height: 0; position: relative; }
  canvas { display: block; cursor: col-resize; touch-action: none; }
  .tip {
    position: fixed; z-index: 20; pointer-events: none; padding: 2px 7px; border-radius: 4px; font-size: 11px; white-space: nowrap;
    transform: translateX(12px); background: var(--panel); color: var(--fg); border: 1px solid var(--border); box-shadow: 0 1px 6px #0003;
  }
  .tip.flip { transform: translateX(calc(-100% - 12px)); }
</style>
