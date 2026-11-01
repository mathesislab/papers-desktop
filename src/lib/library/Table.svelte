<script lang="ts" module>
  import type { Row } from "$lib/types";

  import type { Column } from "./columns";
  import { formatDate } from "./downloads";

  const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

  /** What a column shows of a row, and sorts by. */
  export function value(row: Row, column: Column): string | number | null {
    switch (column) {
      case "tags":
        return row.tags.join(", ");
      case "bib":
        return fileName(row.bib);
      case "added":
        return row.added ?? null;
      default:
        return row[column];
    }
  }

  /** What a cell shows: its value, a date as the Downloads panel writes it. */
  export function text(row: Row, column: Column): string {
    if (column === "added") return row.added ? formatDate(row.added) : "";
    return String(value(row, column) ?? "");
  }
</script>

<script lang="ts">
  // The table of the library tab: only the rows in view drawn; the headers sort, move, resize, show and hide the
  // columns (TableHead: the title, else the last column, takes the room left); a row pressed and dragged goes to the
  // Open section of the sidebar.

  import { gridOf, widthOf, type Shown } from "$lib/list/columns";
  import TableHead from "$lib/list/TableHead.svelte";
  import { ROW, Rows } from "$lib/list/virtual.svelte";
  import { COLUMNS, type Sort } from "./columns";
  import { DRAG_DISTANCE, endDrag, overDrop, rowDrag } from "./drag.svelte";

  let {
    rows,
    columns,
    selected,
    focus,
    sort,
    scroll = 0,
    onscrolled,
    oncolumns,
    onsort,
    onselect,
    onclickrow,
    onopen,
    onmenu,
    ondrop,
  }: {
    /** the rows shown, in order */
    rows: Row[];
    /** the columns shown, in order */
    columns: Shown<Column>[];
    selected: Set<string>;
    /** the row the details show, and that the keys move from */
    focus: string | null;
    sort: Sort;
    /** where the rows are scrolled to when the table comes, px: the library tab as it was left */
    scroll?: number;
    /** the table scrolled, to where */
    onscrolled?: (top: number) => void;
    oncolumns: (columns: Shown<Column>[]) => void;
    onsort: (sort: Sort) => void;
    onselect: (id: string, e: MouseEvent) => void;
    /** a click that did not start a drag, after the selection was left as it was (see `onselect`) */
    onclickrow: (id: string) => void;
    /** Enter-like: the papers to open, a double-click on one of several selected being those selected */
    onopen: (ids: string[]) => void;
    /** right-click on a row: its context menu */
    onmenu: (id: string, e: MouseEvent) => void;
    /** the rows dragged and dropped on the Open section of the sidebar */
    ondrop: (ids: string[]) => void;
  } = $props();

  /** the file marker, before the columns */
  const LEAD = [26];

  let template = $derived(gridOf(LEAD, columns, COLUMNS));
  let minWidth = $derived(widthOf(LEAD, columns, COLUMNS));
  /** only the rows in view drawn, back where they were left; the one in focus scrolled into view */
  const view = new Rows(
    () => rows.length,
    () => (focus ? rows.findIndex((r) => r.id === focus) : -1),
    () => scroll,
  );

  /** the papers selected before the first click of a double-click, which the second opens together */
  let before: string[] | null = null;
  /** a click that ends a drag is not a click on the row */
  let dragged = false;

  /** The papers of `ids`, in the order of the table. */
  const inOrder = (ids: Iterable<string>) => {
    const set = new Set(ids);
    return rows.filter((r) => set.has(r.id)).map((r) => r.id);
  };

  function rowDown(e: MouseEvent, id: string) {
    if (e.detail === 1) before = selected.has(id) && selected.size > 1 ? inOrder(selected) : null;
    onselect(id, e);
  }

  function rowClick(id: string) {
    if (!dragged) onclickrow(id);
  }

  function rowOpen(id: string) {
    onopen(before?.includes(id) ? before : [id]);
  }

  /** A row pressed: dragged far enough, the rows are dragged (the selection when the row is in it, else the
   *  row), to drop on the Open section of the sidebar. */
  function rowPress(e: PointerEvent, id: string) {
    if (e.button !== 0 || e.ctrlKey) return;
    const [x, y] = [e.clientX, e.clientY];
    let ids: string[] | null = null;
    const move = (m: PointerEvent) => {
      if (!ids) {
        if (Math.hypot(m.clientX - x, m.clientY - y) < DRAG_DISTANCE) return;
        ids = inOrder(selected.has(id) ? selected : [id]);
        rowDrag.ids = ids;
      }
      [rowDrag.x, rowDrag.y] = [m.clientX, m.clientY];
      rowDrag.over = overDrop(m.clientX, m.clientY);
    };
    const finish = (drop: boolean) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", key, true);
      if (!ids) return;
      const dropped = drop && rowDrag.over ? ids : null;
      endDrag();
      // the click that follows the release is not a click on a row
      dragged = true;
      setTimeout(() => (dragged = false), 0);
      if (dropped) ondrop(dropped);
    };
    const up = () => finish(true);
    const cancel = () => finish(false);
    const key = (k: KeyboardEvent) => {
      if (k.key !== "Escape") return;
      k.stopPropagation();
      finish(false);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", key, true);
  }

  /** A click: ascending, descending, then the order of the library. Years
   *  and dates added start with the newest. */
  function sortBy(column: Column) {
    const firstDescending = column === "year" || column === "added";
    if (sort?.column !== column) onsort({ column, descending: firstDescending });
    else if (sort.descending === firstDescending) onsort({ column, descending: !firstDescending });
    else onsort(null);
  }

  /** A header: its arrow when the table is sorted by it. */
  const heading = (column: Column) => ({
    arrow: sort?.column === column ? (sort.descending ? "▾" : "▴") : undefined,
    sorted: sort?.column === column,
  });
</script>

<div
  class="table"
  role="grid"
  aria-rowcount={rows.length}
  bind:this={view.table}
  bind:clientHeight={view.height}
  bind:clientWidth={view.width}
  onscroll={() => onscrolled?.(view.scrolled())}
>
  <TableHead lead={LEAD} room={view.width} {columns} specs={COLUMNS} menu={Object.keys(COLUMNS) as Column[]} {heading} onclick={sortBy} {oncolumns} />
  <div class="rows" style:height="{rows.length * ROW}px" style:min-width="{minWidth}px">
    {#each rows.slice(view.first, view.last) as row, i (row.id)}
      <!-- svelte-ignore a11y_click_events_have_key_events (the keys of the window move the selection) -->
      <div
        class="row"
        class:selected={selected.has(row.id)}
        role="row"
        tabindex="-1"
        aria-selected={selected.has(row.id)}
        style:top="{(view.first + i) * ROW}px"
        style:grid-template-columns={template}
        onpointerdown={(e) => rowPress(e, row.id)}
        onmousedown={(e) => rowDown(e, row.id)}
        onclick={() => rowClick(row.id)}
        ondblclick={() => rowOpen(row.id)}
        oncontextmenu={(e) => {
          e.preventDefault();
          onmenu(row.id, e);
        }}
      >
        <span class="file" title={row.file === "missing" ? "File not found" : ""}>
          {#if row.file !== "none"}
            <svg class:missing={row.file === "missing"} viewBox="0 0 12 14" aria-hidden="true">
              <path d="M2 1h5l3 3v9H2z" fill="none" stroke="currentColor" stroke-width="1.2" />
            </svg>
          {/if}
        </span>
        {#each columns as { key } (key)}
          <span class:muted={key === "tags" || key === "kind" || key === "bib" || key === "added"}>{text(row, key)}</span>
        {/each}
      </div>
    {/each}
  </div>
</div>

{#if rowDrag.ids.length}
  <!-- what is dragged, next to the pointer -->
  <div class="ghost" style:left="{rowDrag.x + 12}px" style:top="{rowDrag.y + 12}px">
    {rowDrag.ids.length === 1 ? (rows.find((r) => r.id === rowDrag.ids[0])?.title ?? "1 paper") : `${rowDrag.ids.length} papers`}
  </div>
{/if}

<style>
  .ghost {
    position: fixed;
    z-index: 20;
    max-width: 260px;
    padding: 3px 10px;
    border-radius: 6px;
    background: var(--accent);
    color: var(--on-accent);
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    pointer-events: none;
  }
  /* one scroll, down and across: the headers and the rows keep their columns together */
  .table {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .row {
    display: grid;
    align-items: center;
    height: 26px;
  }
  .rows {
    position: relative;
  }
  .rows .row {
    position: absolute;
    left: 0;
    right: 0;
    border-bottom: 1px solid var(--line);
  }
  .rows .row span {
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
