<script lang="ts">
  /** The bar between two panes, dragged to resize the pane on its `side`
   *  within `min` and `max` pixels. */
  let {
    width = $bindable(),
    side,
    min,
    max,
  }: { width: number; side: "left" | "right"; min: number; max: number } = $props();

  /** where the drag started, and the width then */
  let from: { x: number; width: number } | null = null;

  function down(e: PointerEvent) {
    from = { x: e.clientX, width };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function move(e: PointerEvent) {
    if (!from) return;
    const dx = e.clientX - from.x;
    width = Math.round(Math.min(max, Math.max(min, from.width + (side === "left" ? dx : -dx))));
  }
</script>

<div
  class="splitter"
  role="separator"
  aria-orientation="vertical"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={() => (from = null)}
></div>

<style>
  .splitter {
    flex: none;
    width: 5px;
    margin: 0 -2px;
    z-index: 1;
    cursor: col-resize;
  }
</style>
