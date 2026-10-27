<script lang="ts" module>
  export type Column = "authors" | "year" | "title" | "container" | "tags";
  /** a column and its direction; none: the order of the library (newest first) */
  export type Sort = { column: Column; descending: boolean } | null;
</script>

<script lang="ts">
  import type { Row } from "$lib/types";

  let {
    rows,
    selected,
    focus,
    sort,
    onsort,
    onselect,
    onopen,
  }: {
    /** the rows shown, in order */
    rows: Row[];
    selected: Set<string>;
    /** the row the details show, and that the keys move from */
    focus: string | null;
    sort: Sort;
    onsort: (sort: Sort) => void;
    onselect: (id: string, e: MouseEvent) => void;
    onopen: (id: string) => void;
  } = $props();

  const ROW = 26;
  /** rows drawn above and below the ones in view, for a smooth scroll */
  const EXTRA = 12;

  const columns: [Column, string][] = [
    ["authors", "Authors"],
    ["year", "Year"],
    ["title", "Title"],
    ["container", "Journal"],
    ["tags", "Tags"],
  ];

  let body = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(0);

  // only the rows in view are drawn
  let first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - EXTRA));
  let last = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + EXTRA));

  // the focused row is scrolled into view (moved to with the keys)
  $effect(() => {
    const index = focus ? rows.findIndex((r) => r.id === focus) : -1;
    if (index < 0 || !body) return;
    const top = index * ROW;
    if (top < body.scrollTop) body.scrollTop = top;
    else if (top + ROW > body.scrollTop + body.clientHeight) body.scrollTop = top + ROW - body.clientHeight;
  });

  /** A click: ascending, descending, then the order of the library. Years
   *  start with the newest. */
  function sortBy(column: Column) {
    const firstDescending = column === "year";
    if (sort?.column !== column) onsort({ column, descending: firstDescending });
    else if (sort.descending === firstDescending) onsort({ column, descending: !firstDescending });
    else onsort(null);
  }
</script>

<div class="table" role="grid" aria-rowcount={rows.length}>
  <div class="head row" role="row">
    <span class="file" aria-label="File"></span>
    {#each columns as [column, name]}
      <button role="columnheader" class:sorted={sort?.column === column} onclick={() => sortBy(column)}>
        {name}{#if sort?.column === column}<span class="arrow">{sort.descending ? "▾" : "▴"}</span>{/if}
      </button>
    {/each}
  </div>
  <div
    class="body"
    bind:this={body}
    bind:clientHeight={height}
    onscroll={() => (scrollTop = body!.scrollTop)}
  >
    <div style:height="{rows.length * ROW}px" style:position="relative">
      {#each rows.slice(first, last) as row, i (row.id)}
        <!-- svelte-ignore a11y_click_events_have_key_events (the keys of the window move the selection) -->
        <div
          class="row"
          class:selected={selected.has(row.id)}
          role="row"
          tabindex="-1"
          aria-selected={selected.has(row.id)}
          style:top="{(first + i) * ROW}px"
          onmousedown={(e) => onselect(row.id, e)}
          ondblclick={() => onopen(row.id)}
        >
          <span class="file" title={row.file === "missing" ? "File not found" : ""}>
            {#if row.file !== "none"}
              <svg class:missing={row.file === "missing"} viewBox="0 0 12 14" aria-hidden="true">
                <path d="M2 1h5l3 3v9H2z" fill="none" stroke="currentColor" stroke-width="1.2" />
              </svg>
            {/if}
          </span>
          <span>{row.authors}</span>
          <span class="year">{row.year ?? ""}</span>
          <span>{row.title}</span>
          <span>{row.container ?? ""}</span>
          <span class="tags">{row.tags.join(", ")}</span>
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .table {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .row {
    display: grid;
    grid-template-columns: 26px minmax(120px, 1.1fr) 52px minmax(160px, 3fr) minmax(90px, 1.1fr) minmax(60px, 0.7fr);
    align-items: center;
    height: 26px;
  }
  .head {
    flex: none;
    border-bottom: 1px solid var(--line);
  }
  .head button {
    height: 100%;
    padding: 0 8px;
    border: none;
    background: none;
    color: var(--muted);
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    text-align: left;
    white-space: nowrap;
    cursor: default;
  }
  .head button.sorted {
    color: var(--fg);
  }
  .arrow {
    margin-left: 4px;
    font-size: 10px;
  }
  .body {
    flex: 1;
    overflow-y: auto;
  }
  .body .row {
    position: absolute;
    left: 0;
    right: 0;
    border-bottom: 1px solid var(--line);
  }
  .body .row span {
    padding: 0 8px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .row.selected {
    background: var(--selection);
    color: var(--on-selection);
  }
  .year {
    font-variant-numeric: tabular-nums;
  }
  .tags {
    color: var(--muted);
  }
  .row.selected .tags {
    color: inherit;
  }
  .file {
    display: flex;
    justify-content: center;
  }
  .file svg {
    width: 11px;
    height: 13px;
    color: var(--accent);
  }
  .row.selected .file svg {
    color: inherit;
  }
  .file svg.missing {
    color: var(--error);
  }
</style>
