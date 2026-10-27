<script lang="ts" module>
  import type { Row } from "$lib/types";

  export type Column = "authors" | "year" | "title" | "container" | "tags" | "key" | "kind" | "doi" | "bib";
  /** a column shown, and its width in pixels */
  export type Shown = { column: Column; width: number };
  /** a column and its direction; none: the order of the library (newest first) */
  export type Sort = { column: Column; descending: boolean } | null;

  /** the columns, in the order of the menu, with their width at first */
  export const COLUMNS: Record<Column, { name: string; width: number }> = {
    authors: { name: "Authors", width: 150 },
    year: { name: "Year", width: 56 },
    title: { name: "Title", width: 320 },
    container: { name: "Journal", width: 170 },
    tags: { name: "Tags", width: 120 },
    key: { name: "Key", width: 140 },
    kind: { name: "Type", width: 90 },
    doi: { name: "DOI", width: 190 },
    bib: { name: "Library", width: 120 },
  };

  export const DEFAULT_COLUMNS: Shown[] = (["authors", "year", "title", "container", "tags"] as Column[]).map(
    (column) => ({ column, width: COLUMNS[column].width }),
  );

  const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

  /** What a column shows of a row, and sorts by. */
  export function value(row: Row, column: Column): string | number | null {
    switch (column) {
      case "tags":
        return row.tags.join(", ");
      case "bib":
        return fileName(row.bib);
      default:
        return row[column];
    }
  }
</script>

<script lang="ts">
  import { CheckMenuItem, Menu } from "@tauri-apps/api/menu";

  let {
    rows,
    columns,
    selected,
    focus,
    sort,
    oncolumns,
    onsort,
    onselect,
    onopen,
  }: {
    /** the rows shown, in order */
    rows: Row[];
    /** the columns shown, in order */
    columns: Shown[];
    selected: Set<string>;
    /** the row the details show, and that the keys move from */
    focus: string | null;
    sort: Sort;
    oncolumns: (columns: Shown[]) => void;
    onsort: (sort: Sort) => void;
    onselect: (id: string, e: MouseEvent) => void;
    onopen: (id: string) => void;
  } = $props();

  const ROW = 26;
  /** rows drawn above and below the ones in view, for a smooth scroll */
  const EXTRA = 12;
  const MIN_WIDTH = 40;
  /** a header moved this far is dragged, not clicked */
  const DRAG = 4;

  let body = $state<HTMLDivElement>();
  let head = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(0);

  // only the rows in view are drawn
  let first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - EXTRA));
  let last = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + EXTRA));

  // the title takes the room left, else the last column
  let flexible = $derived(columns.some((c) => c.column === "title") ? "title" : columns.at(-1)?.column);
  let template = $derived(
    ["26px", ...columns.map((c) => (c.column === flexible ? `minmax(${c.width}px, 1fr)` : `${c.width}px`))].join(" "),
  );
  let minWidth = $derived(26 + columns.reduce((sum, c) => sum + c.width, 0));

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

  /** a header pressed: dragged to move its column, or clicked to sort by it */
  let press: { index: number; x: number; dragging: boolean } | null = null;
  /** where the column dragged would go: before the column of this index */
  let dropAt = $state<number | null>(null);

  function headerDown(e: PointerEvent, index: number) {
    if (e.button !== 0) return;
    press = { index, x: e.clientX, dragging: false };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function headerMove(e: PointerEvent) {
    if (!press || !head) return;
    if (!press.dragging && Math.abs(e.clientX - press.x) < DRAG) return;
    press.dragging = true;
    const cells = [...head.querySelectorAll<HTMLElement>("[data-column]")];
    const at = cells.findIndex((cell) => {
      const box = cell.getBoundingClientRect();
      return e.clientX < box.left + box.width / 2;
    });
    dropAt = at < 0 ? cells.length : at;
  }

  function headerUp(column: Column) {
    if (!press) return;
    if (!press.dragging) {
      sortBy(column);
    } else if (dropAt !== null && dropAt !== press.index && dropAt !== press.index + 1) {
      const moved = [...columns];
      const [c] = moved.splice(press.index, 1);
      moved.splice(dropAt > press.index ? dropAt - 1 : dropAt, 0, c);
      oncolumns(moved);
    }
    press = null;
    dropAt = null;
  }

  /** the edge of a header dragged: its column resized */
  let resizing: { index: number; x: number; width: number } | null = null;

  function edgeDown(e: PointerEvent, index: number) {
    if (e.button !== 0) return;
    e.stopPropagation();
    resizing = { index, x: e.clientX, width: columns[index].width };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function edgeMove(e: PointerEvent) {
    if (!resizing) return;
    const { index, x, width } = resizing;
    const resized = [...columns];
    resized[index] = { ...resized[index], width: Math.max(MIN_WIDTH, Math.round(width + e.clientX - x)) };
    oncolumns(resized);
  }

  /** Right-click on the headers: the columns to show. */
  async function columnMenu(e: MouseEvent) {
    e.preventDefault();
    const items = await Promise.all(
      (Object.keys(COLUMNS) as Column[]).map((column) => {
        const on = columns.some((c) => c.column === column);
        return CheckMenuItem.new({
          text: COLUMNS[column].name,
          checked: on,
          // the last column stays
          enabled: !on || columns.length > 1,
          action: () =>
            oncolumns(
              on
                ? columns.filter((c) => c.column !== column)
                : [...columns, { column, width: COLUMNS[column].width }],
            ),
        });
      }),
    );
    const menu = await Menu.new({ items });
    await menu.popup();
  }
</script>

<div class="table" role="grid" aria-rowcount={rows.length}>
  <div
    class="head row"
    role="row"
    tabindex="-1"
    bind:this={head}
    style:grid-template-columns={template}
    style:min-width="{minWidth}px"
    oncontextmenu={columnMenu}
  >
    <span class="file" aria-label="File"></span>
    {#each columns as { column }, i (column)}
      <div
        class="cell"
        class:sorted={sort?.column === column}
        class:drop-before={dropAt === i}
        class:drop-after={dropAt === columns.length && i === columns.length - 1}
        role="columnheader"
        tabindex="-1"
        data-column={column}
        onpointerdown={(e) => headerDown(e, i)}
        onpointermove={headerMove}
        onpointerup={() => headerUp(column)}
      >
        <span class="name">{COLUMNS[column].name}</span>
        {#if sort?.column === column}<span class="arrow">{sort.descending ? "▾" : "▴"}</span>{/if}
        <span
          class="edge"
          role="separator"
          aria-orientation="vertical"
          onpointerdown={(e) => edgeDown(e, i)}
          onpointermove={edgeMove}
          onpointerup={() => (resizing = null)}
        ></span>
      </div>
    {/each}
  </div>
  <div
    class="body"
    bind:this={body}
    bind:clientHeight={height}
    style:min-width="{minWidth}px"
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
          style:grid-template-columns={template}
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
          {#each columns as { column } (column)}
            <span class:muted={column === "tags" || column === "kind" || column === "bib"}>{value(row, column) ?? ""}</span>
          {/each}
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
    overflow-x: auto;
  }
  .row {
    display: grid;
    align-items: center;
    height: 26px;
  }
  .head {
    flex: none;
    border-bottom: 1px solid var(--line);
  }
  .cell {
    position: relative;
    display: flex;
    align-items: center;
    height: 100%;
    min-width: 0;
    padding: 0 8px;
    color: var(--muted);
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
  }
  .cell.sorted {
    color: var(--fg);
  }
  .cell .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cell.drop-before {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .cell.drop-after {
    box-shadow: inset -2px 0 0 var(--accent);
  }
  .arrow {
    margin-left: 4px;
    font-size: 10px;
  }
  .edge {
    position: absolute;
    top: 4px;
    right: -3px;
    bottom: 4px;
    z-index: 1;
    width: 6px;
    cursor: col-resize;
  }
  .edge::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 2px;
    width: 1px;
    background: var(--line);
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
  .muted {
    color: var(--muted);
  }
  .row.selected .muted {
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
