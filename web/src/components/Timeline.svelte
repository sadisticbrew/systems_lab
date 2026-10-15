<script lang="ts">
  import { lab } from "../lib/state.svelte";

  let canvas: HTMLCanvasElement;
  let wrap: HTMLDivElement;
  let width = $state(0);
  let height = $state(0);
  let dragging = false;
  let target: number | null = null;
  let busy = false;

  $effect(() => {
    const ro = new ResizeObserver(() => {
      width = wrap.clientWidth;
      height = wrap.clientHeight;
    });
    ro.observe(wrap);
    return () => ro.disconnect();
  });

  const css = (n: string) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();

  function draw() {
    const tl = lab.timeline;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.max(1, Math.floor(width * dpr));
    canvas.height = Math.max(1, Math.floor(height * dpr));
    const g = canvas.getContext("2d")!;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, width, height);
    if (!tl || width === 0) return;
    const N = Math.max(1, tl.steps);
    const maxd = Math.max(1, ...tl.depth);
    g.fillStyle = css("--sil");
    const bw = width / Math.max(1, tl.depth.length);
    tl.depth.forEach((d, i) => {
      const h = (d / maxd) * (height - 6);
      g.fillRect(i * bw, height - h, Math.ceil(bw), h);
    });
    const colors: Record<string, string> = {
      call: css("--c1"), ret: css("--c2"), syscall: css("--c3"), signal: css("--red"), abi: css("--orange"),
    };
    for (const m of tl.markers) {
      g.fillStyle = colors[m.kind] ?? "#888";
      const x = Math.floor((m.step / N) * width);
      const tall = m.kind === "signal" || m.kind === "abi" || m.kind === "syscall";
      g.fillRect(x, tall ? 0 : height - 10, 1, tall ? height : 10);
    }
    g.fillStyle = css("--fg");
    g.fillRect(Math.round((lab.step / N) * width) - 1, 0, 2, height);
  }

  $effect(() => {
    // redraw whenever any input changes
    void [lab.timeline, lab.step, width, height];
    if (canvas) draw();
  });

  async function pump() {
    if (busy) return;
    busy = true;
    while (target !== null) {
      const t = target;
      target = null;
      await lab.goto(t);
    }
    busy = false;
  }

  function scrub(e: PointerEvent) {
    const r = canvas.getBoundingClientRect();
    const x = Math.max(0, Math.min(r.width, e.clientX - r.left));
    target = Math.round((x / r.width) * lab.last);
    pump();
  }
</script>

<div class="tl panel" bind:this={wrap} title="click or drag to scrub">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <canvas
    bind:this={canvas}
    style="width:{width}px;height:{height}px"
    onpointerdown={(e) => { dragging = true; canvas.setPointerCapture(e.pointerId); scrub(e); }}
    onpointermove={(e) => { if (dragging) scrub(e); }}
    onpointerup={() => (dragging = false)}
    onpointercancel={() => (dragging = false)}
  ></canvas>
</div>

<style>
  .tl { position: relative; }
  canvas { display: block; cursor: col-resize; touch-action: none; }
</style>
