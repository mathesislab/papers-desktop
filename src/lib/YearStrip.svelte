<script lang="ts">
  import type { YearCount } from "$lib/types";

  let {
    years,
    onyear,
    onrange,
    onclear,
  }: {
    years: YearCount[];
    /** a click on a year, with its modifier keys */
    onyear: (year: number, e: MouseEvent) => void;
    /** a drag across the years */
    onrange: (from: number, to: number) => void;
    /** the button that drops the year filters */
    onclear: () => void;
  } = $props();

  let bars: HTMLDivElement;
  /** the years dragged across, as indices */
  let drag = $state<{ from: number; to: number } | null>(null);
  let hover = $state<number | null>(null);

  let max = $derived(Math.max(1, ...years.map((y) => y.count)));
  let filtered = $derived(years.some((y) => !y.selected));
  let dragged = $derived(drag && [Math.min(drag.from, drag.to), Math.max(drag.from, drag.to)]);

  let info = $derived.by(() => {
    if (dragged && dragged[0] !== dragged[1]) {
      const [a, b] = dragged;
      const count = years.slice(a, b + 1).reduce((n, y) => n + y.count, 0);
      return `${years[a].year}–${years[b].year} · ${count}`;
    }
    if (hover !== null) return `${years[hover].year} · ${years[hover].count}`;
    return "Drag across years · click one · ⇧ extend · ⌥ exclude";
  });

  function index(e: PointerEvent): number {
    const box = bars.getBoundingClientRect();
    const i = Math.floor(((e.clientX - box.left) / box.width) * years.length);
    return Math.max(0, Math.min(years.length - 1, i));
  }

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    bars.setPointerCapture(e.pointerId);
    const i = index(e);
    drag = { from: i, to: i };
  }

  function move(e: PointerEvent) {
    hover = index(e);
    if (drag) drag = { ...drag, to: hover };
  }

  function up(e: PointerEvent) {
    if (!dragged) return;
    const [a, b] = dragged;
    drag = null;
    if (a === b) onyear(years[a].year, e);
    else onrange(years[a].year, years[b].year);
  }
</script>

<div class="strip">
  <!-- the search field keeps the focus -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="bars"
    class:filtered
    bind:this={bars}
    onmousedown={(e) => e.preventDefault()}
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointerleave={() => (hover = null)}
  >
    {#each years as y, i (y.year)}
      <div
        class="bar"
        class:selected={y.selected}
        class:dragged={dragged && i >= dragged[0] && i <= dragged[1]}
        class:hovered={hover === i}
      >
        <div class="fill" style:height="{(y.count / max) * 100}%"></div>
      </div>
    {/each}
  </div>
  <div class="axis">
    <span>{years[0].year}</span>
    <span class="info">{info}</span>
    {#if filtered}
      <button class="clear" onmousedown={(e) => e.preventDefault()} onclick={onclear}>All years ✕</button>
    {/if}
    <span>{years[years.length - 1].year}</span>
  </div>
</div>

<style>
  .strip {
    flex: none;
    padding: 8px 18px 4px;
    border-bottom: 1px solid var(--line);
  }
  .bars {
    display: flex;
    align-items: stretch;
    height: 28px;
    cursor: crosshair;
  }
  .bar {
    flex: 1;
    display: flex;
    align-items: flex-end;
    padding: 0 0.5px;
  }
  .fill {
    width: 100%;
    border-radius: 1px 1px 0 0;
    background: var(--muted);
    opacity: 0.45;
  }
  .filtered .bar.selected .fill {
    background: var(--accent);
    opacity: 0.9;
  }
  .filtered .bar:not(.selected) .fill {
    opacity: 0.2;
  }
  .bar.dragged .fill {
    background: var(--accent);
    opacity: 0.6;
  }
  .bar.hovered .fill {
    opacity: 1;
  }
  .axis {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    margin-top: 3px;
    color: var(--muted);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }
  .info {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    text-align: center;
  }
  .clear {
    flex: none;
    padding: 0 6px;
    border: 1px solid var(--line);
    border-radius: 4px;
    background: transparent;
    color: var(--fg);
    font: inherit;
    cursor: pointer;
  }
  .clear:hover {
    border-color: var(--muted);
  }
</style>
