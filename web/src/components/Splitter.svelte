<script lang="ts">
  // A 6px drag handle. `ondrag` gets the distance in px from where the drag started, so the parent
  // computes sizes from the values it captured in `onstart` and clamping never drifts.
  let {
    orientation,
    label,
    onstart,
    ondrag,
    onend,
    onreset,
  }: {
    /** `vertical` separates left from right (drag sideways); `horizontal` separates top from bottom. */
    orientation: "vertical" | "horizontal";
    label: string;
    onstart?: () => void;
    ondrag: (delta: number) => void;
    onend?: () => void;
    /** Double-click. */
    onreset: () => void;
  } = $props();

  let dragging = $state(false);
  let moved = false;
  let origin = 0;

  const coord = (e: PointerEvent) => (orientation === "vertical" ? e.clientX : e.clientY);

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    origin = coord(e);
    dragging = true;
    moved = false;
    onstart?.();
    e.preventDefault();
  }

  function move(e: PointerEvent) {
    if (!dragging) return;
    moved = true;
    ondrag(coord(e) - origin);
  }

  function up() {
    if (!dragging) return;
    dragging = false;
    if (moved) onend?.();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="split {orientation}"
  class:dragging
  role="separator"
  aria-orientation={orientation}
  aria-label={label}
  title="Drag to resize, double-click to reset"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  ondblclick={onreset}
></div>

<style>
  .split { position: relative; flex: none; touch-action: none; }
  .split.vertical { width: 6px; cursor: col-resize; }
  .split.horizontal { height: 6px; cursor: row-resize; }
  .split::after { content: ""; position: absolute; border-radius: 2px; background: transparent; transition: background .12s; }
  .split.vertical::after { top: 0; bottom: 0; left: 2px; width: 2px; }
  .split.horizontal::after { left: 0; right: 0; top: 2px; height: 2px; }
  .split:hover::after, .split.dragging::after { background: var(--accent); }
</style>
