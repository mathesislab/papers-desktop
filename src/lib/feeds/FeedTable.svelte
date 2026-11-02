<script lang="ts">
  // The list of the Feeds tab: the items the filters select, best score or newest first; and of the Search tab: the
  // works found, in the order OpenAlex gave them (the caller chooses the columns). The score is two bars, its
  // keywords over its library (ScoreBars), in its column and, small, under the date of a simple row.
  // Only the rows in view are drawn, as in the library's table; the headers move, resize, show and hide the columns
  // as the library's do (TableHead: the title takes the room left; the window's layout keeps them). Simple
  // (`look`, the Feeds tab's at first; SimpleRow): no headers, each row taller, its date (an age; in the Search tab,
  // its year) over its score, then its source, title and authors a line each, its status beside its source. The
  // status is symbols (Marks), in their slots in its column. A new item's title is bold; one seen is muted (in the
  // Feeds tab). A click selects (⇧ a range, ⌘ one more), a double-click opens its details, a right-click its menu; the star at
  // its start stars the item, or unstars it (✓: added); in the Feeds tab, the ✕ at its end, on hover and on the row
  // selected, removes it (or restores it, removed).

  import type { Snippet } from "svelte";
  import { gridOf, simpleGridOf, widthOf, type Look, type Shown, type Specs } from "$lib/list/columns";
  import SimpleRow from "$lib/list/SimpleRow.svelte";
  import TableHead from "$lib/list/TableHead.svelte";
  import { Rows } from "$lib/list/virtual.svelte";
  import Marked from "$lib/Marked.svelte";
  import { STAR, rowHint, type Action } from "./actions";
  import type { FeedRow, Sort } from "./api";
  import type { FeedColumn } from "./columns";
  import { ago, citedText, marks, yearOf } from "./items";
  import { ITEM_KINDS, isPost } from "./kinds";
  import Marks from "./Marks.svelte";
  import ScoreBars from "./ScoreBars.svelte";

  let {
    rows,
    selected,
    focus,
    columns,
    specs,
    menu,
    sort,
    libraryOf,
    look = "table",
    date = "age",
    unread = true,
    star = STAR.row,
    remove,
    after,
    onsort,
    oncolumns,
    onstar,
    onselect,
    onopen,
    onmenu,
  }: {
    rows: FeedRow[];
    selected: Set<string>;
    /** the row the details show, and that the keys move from */
    focus: string | null;
    /** the columns shown after the star, in their order, with their widths */
    columns: Shown<FeedColumn>[];
    /** the tab's columns: their names, widths at first, least widths; those the menu of the headers offers */
    specs: Specs<FeedColumn>;
    menu: FeedColumn[];
    /** the order of the list, its header marked; none: the headers do not sort */
    sort?: Sort;
    /** the .bib of a paper of the library, by its name, when it has several: the status of an item in it */
    libraryOf: (id: string) => string | null;
    /** a table, or simple */
    look?: Look;
    /** what the simple list says of the date: an age ("5 min", the Feeds tab's), or the year (the Search tab's, as
     *  its table's Year column) */
    date?: "age" | "year";
    /** a new item's title bold, one seen muted */
    unread?: boolean;
    /** what the star of a row says, on and off (the tab's `StarWords`) */
    star?: { on: string; off: string };
    /** the ✕ of a row: what it does to the item (the Feeds tab's remove); none: no ✕ */
    remove?: Action;
    /** after the last row, in the scroll of the list: *Show 25 more* */
    after?: Snippet;
    onsort?: (sort: Sort) => void;
    oncolumns: (columns: Shown<FeedColumn>[]) => void;
    /** the star of a row clicked: the item starred, or no longer */
    onstar: (id: string) => void;
    onselect: (id: string, e: MouseEvent) => void;
    onopen: (id: string) => void;
    /** right-click on a row: its context menu */
    onmenu: (id: string) => void;
  } = $props();

  /** the star, before the columns */
  const LEAD = [22];
  const SIMPLE_GRID = simpleGridOf(LEAD);
  /** the columns that sort the list, by what, and what their headers say on hover */
  const SORTS: Partial<Record<FeedColumn, { sort: Sort; title: string }>> = {
    score: { sort: "score", title: "Best score first" },
    date: { sort: "newest", title: "Newest first" },
  };

  let now = $state(Date.now());
  let template = $derived(gridOf(LEAD, columns, specs));
  let minWidth = $derived(widthOf(LEAD, columns, specs));
  /** only the rows in view drawn; the one in focus scrolled into view */
  const view = new Rows({
    count: () => rows.length,
    focus: () => (focus ? rows.findIndex((r) => r.id === focus) : -1),
    look: () => look,
  });

  /** A header: its name, its arrow when the list is sorted by it; on hover, what the column is and the order a
   *  click gives. */
  function heading(column: FeedColumn) {
    const by = SORTS[column];
    const sorted = !!by && sort === by.sort;
    const title = [specs[column].hint, onsort ? by?.title : undefined].filter(Boolean).join("\n");
    // the symbols speak for themselves: no name over them
    return { name: column === "status" ? "" : undefined, arrow: sorted ? "▾" : undefined, title: title || undefined, sorted };
  }

  /** A header clicked: the list sorted by it, if it sorts. */
  function sortBy(column: FeedColumn) {
    const by = SORTS[column];
    if (by && onsort) onsort(by.sort);
  }

  // the ages ("5 min") move on
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 60000);
    return () => clearInterval(timer);
  });

  /** The family names of a row: three, then "et al." */
  const authorsOf = (row: FeedRow) => row.authors.slice(0, 3);

  /** The journal of a row, else the first subscription that listed it. */
  const sourceOf = (row: FeedRow) => row.journal ?? row.sources[0] ?? "";
</script>

<!-- the authors of a row, a post's kind without -->
{#snippet names(row: FeedRow)}
  {#if row.authors.length}
    {#each authorsOf(row) as name, j}{#if j}{", "}{/if}<Marked spans={name} />{/each}{#if row.authors.length > 3}, et al.{/if}
  {:else if isPost(row.kind)}
    {ITEM_KINDS[row.kind].name}
  {/if}
{/snippet}

<div
  class="table"
  class:simple={view.simple}
  role="grid"
  aria-rowcount={rows.length}
  bind:this={view.table}
  bind:clientHeight={view.height}
  bind:clientWidth={view.width}
  onscroll={view.scrolled}
>
  {#if !view.simple}
    <TableHead lead={LEAD} room={view.width} {columns} {specs} {menu} {heading} onclick={sortBy} {oncolumns} />
  {/if}
  <div
    class="rows"
    style:height="{rows.length * view.row}px"
    style:min-width={view.simple ? null : `${minWidth}px`}
    style:--end="{view.scrollLeft + view.width}px"
  >
    {#each rows.slice(view.first, view.last) as row, i (row.id)}
      <!-- svelte-ignore a11y_click_events_have_key_events (the keys of the window move the selection) -->
      <div
        class="row {unread ? row.triage : ''}"
        class:selected={selected.has(row.id)}
        role="row"
        tabindex="-1"
        aria-selected={selected.has(row.id)}
        style:top="{(view.first + i) * view.row}px"
        style:height="{view.row}px"
        style:grid-template-columns={view.simple ? SIMPLE_GRID : template}
        onmousedown={(e) => onselect(row.id, e)}
        ondblclick={() => onopen(row.id)}
        oncontextmenu={(e) => {
          e.preventDefault();
          onmenu(row.id);
        }}
      >
        {#if row.triage === "added"}
          <span class="star added" title="Added">✓</span>
        {:else}
          <button
            class="star"
            class:on={row.triage === "starred"}
            title={row.triage === "starred" ? star.on : star.off}
            onmousedown={(e) => e.stopPropagation()}
            ondblclick={(e) => e.stopPropagation()}
            onclick={() => onstar(row.id)}>{row.triage === "starred" ? "★" : "☆"}</button
          >
        {/if}
        {#if view.simple}
          <SimpleRow
            lead={LEAD.length}
            date={date === "age" ? ago(row.date, now) : yearOf(row.date)}
            source={sourceOf(row)}
            sources={row.sources.join(", ")}
          >
            {#snippet title()}<span class="title"><Marked spans={row.title} /></span>{/snippet}
            {#snippet beside()}<Marks marks={marks(row, libraryOf)} look="inline" />{/snippet}
            {#snippet authors()}{@render names(row)}{/snippet}
            {#snippet under()}<ScoreBars {row} small />{/snippet}
          </SimpleRow>
        {:else}
          {#each columns as { key: column } (column)}
            {#if column === "score"}
              <span class="score"><ScoreBars {row} /></span>
            {:else if column === "cited"}
              <span class="cited muted" title="The works that cite it, as OpenAlex counts them">{citedText(row.cited)}</span>
            {:else if column === "title"}
              <span class="title"><Marked spans={row.title} /></span>
            {:else if column === "authors"}
              <span class="muted">{@render names(row)}</span>
            {:else if column === "journal"}
              <span class="muted" title={row.sources.join(", ")}>{sourceOf(row)}</span>
            {:else if column === "date"}
              <span class="muted date">{ago(row.date, now)}</span>
            {:else if column === "year"}
              <span class="muted date">{yearOf(row.date)}</span>
            {:else}
              <span class="status"><Marks marks={marks(row, libraryOf)} look="slots" /></span>
            {/if}
          {/each}
        {/if}
        {#if remove}
          <button
            class="remove"
            title={rowHint(remove, [row])}
            aria-label={remove.label([row])}
            onmousedown={(e) => e.stopPropagation()}
            ondblclick={(e) => e.stopPropagation()}
            onclick={() => remove.run([row.id])}>✕</button
          >
        {/if}
      </div>
    {/each}
  </div>
  {@render after?.()}
</div>

<style>
  /* one scroll, down and across: the headers and the rows keep their columns together */
  .table {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .row {
    display: grid;
    align-items: center;
  }
  /* simple, the status clear of the ✕ */
  .simple .row:has(.remove) {
    padding-right: 26px;
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
  .rows .row > span {
    padding: 0 8px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .row.new .title {
    font-weight: 600;
  }
  .row.seen,
  .row.removed {
    color: var(--muted);
  }
  .row.removed .title {
    text-decoration: line-through;
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
  .cited,
  .date {
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  /* the count off the edge of the window, when its column is the last */
  .rows .row > .cited:last-child {
    padding-right: 16px;
  }
  .rows .row > .score {
    align-self: stretch;
  }
  /* the symbols' slots fill it: a little room either side */
  .rows .row > .status {
    padding: 0 4px;
  }
  /* simple: the date over the score, under the star */
  .simple .row {
    grid-template-rows: 34px 1fr;
  }
  .rows .row > .star {
    padding: 0;
    text-align: center;
    font-size: 11px;
  }
  .star {
    padding: 0;
    border: none;
    background: none;
    color: var(--border);
    font: inherit;
    font-size: 13px;
    text-align: center;
    cursor: pointer;
  }
  .row:hover .star {
    color: var(--muted);
  }
  .star.on,
  .row:hover .star.on {
    color: #d9822b;
  }
  .star.added {
    color: #2f9e6e;
    cursor: default;
  }
  .row.selected .star {
    color: inherit;
  }
  /* the ✕, over the end of the row in view (`--end`: the table may scroll across), on hover and on the row selected */
  .remove {
    position: absolute;
    top: 50%;
    left: calc(var(--end) - 26px);
    display: none;
    width: 20px;
    height: 20px;
    padding: 0;
    border: none;
    border-radius: 4px;
    background: var(--bg);
    color: var(--muted);
    font: inherit;
    font-size: 11px;
    line-height: 20px;
    text-align: center;
    transform: translateY(-50%);
    cursor: pointer;
  }
  .row:hover .remove,
  .row.selected .remove {
    display: block;
  }
  .row.selected .remove {
    background: var(--selection);
    color: inherit;
  }
  .remove:hover,
  .row.selected .remove:hover {
    background: var(--line);
    color: var(--fg);
  }
</style>
