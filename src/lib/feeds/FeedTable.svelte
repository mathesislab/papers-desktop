<script lang="ts" module>
  /** a column of the list after its mark, which an edge of its header resizes */
  export type FeedColumn = "score" | "cited" | "similar" | "title" | "authors" | "journal" | "date" | "year" | "badges";

  /** the widths of the columns at first, in pixels; the title's is its least (it takes the room left) */
  export const FEED_COLUMNS: Record<FeedColumn, number> = {
    score: 60,
    cited: 64,
    similar: 70,
    title: 240,
    authors: 150,
    journal: 150,
    date: 58,
    year: 50,
    badges: 190,
  };

  /** the columns of the Feeds tab, in their order */
  export const FEED_ORDER: FeedColumn[] = ["score", "similar", "title", "authors", "journal", "date", "badges"];

  /** the columns of the Search tab: the citations first, the year, no score; and their widths at first */
  export const SEARCH_ORDER: FeedColumn[] = ["cited", "similar", "title", "authors", "journal", "year", "badges"];
  export const SEARCH_COLUMNS: Record<FeedColumn, number> = { ...FEED_COLUMNS, title: 280, journal: 170, badges: 110 };
</script>

<script lang="ts">
  // The list of the Feeds tab: the items the filters select, best score, most similar to the library or newest
  // first; and of the Search tab: the works found, in the order OpenAlex gave them (the caller chooses the columns).
  // Only the rows in view are drawn, as in the library's table, and an edge of a header resizes a column, the
  // title taking the room left (the window's layout keeps the widths). A new item's title is bold; one seen is
  // muted (in the Feeds tab). A click selects (⇧ a range, ⌘ one more), a double-click opens the item's page, a
  // right-click its menu; the star at its start stars the item, or unstars it (✓: added).

  import type { Snippet } from "svelte";
  import Marked from "$lib/Marked.svelte";
  import type { FeedRow, Sort } from "./api";
  import { ago, badges, citedText, likeText, scoreText, yearOf } from "./items";
  import { ITEM_KINDS, isPost } from "./kinds";

  let {
    rows,
    selected,
    focus,
    columns = FEED_ORDER,
    names = {},
    sort,
    widths,
    colours,
    unread = true,
    star = { on: "Starred: unstar", off: "Star" },
    after,
    onsort,
    onwidths,
    onstar,
    onselect,
    onopen,
    onmenu,
  }: {
    rows: FeedRow[];
    selected: Set<string>;
    /** the row the details show, and that the keys move from */
    focus: string | null;
    /** the columns shown after the star and the mark, in their order */
    columns?: FeedColumn[];
    /** the names of headers other than the Feeds tab's */
    names?: Partial<Record<FeedColumn, string>>;
    /** the order of the list, its header marked; none: the headers do not sort */
    sort?: Sort;
    widths: Record<FeedColumn, number>;
    /** the colours of the interests */
    colours: Map<string, string>;
    /** a new item's title bold, one seen muted */
    unread?: boolean;
    /** what the star of a row says, on and off */
    star?: { on: string; off: string };
    /** after the last row, in the scroll of the list: *Show 25 more* */
    after?: Snippet;
    onsort?: (sort: Sort) => void;
    onwidths: (widths: Record<FeedColumn, number>) => void;
    /** the star of a row clicked: the item starred, or no longer */
    onstar: (id: string) => void;
    onselect: (id: string, e: MouseEvent) => void;
    onopen: (id: string) => void;
    /** right-click on a row: its context menu */
    onmenu: (id: string) => void;
  } = $props();

  const ROW = 26;
  /** rows drawn above and below the ones in view, for a smooth scroll */
  const EXTRA = 12;
  const STAR = 22;
  const MARK = 18;
  const MIN_WIDTH = 30;
  /** the least widths above MIN_WIDTH: a sorted header and its arrow fit, a title stays readable */
  const LEAST: Partial<Record<FeedColumn, number>> = { score: 60, cited: 56, similar: 70, date: 58, year: 48, title: 100 };
  const least = (column: FeedColumn) => LEAST[column] ?? MIN_WIDTH;
  /** the headers, and the order each sorts by (none: it does not sort) */
  const HEADERS: Record<FeedColumn, { name: string; sort?: Sort; title?: string }> = {
    score: { name: "Score", sort: "score", title: "Best score first" },
    cited: { name: "Cited" },
    similar: { name: "Similar", sort: "like", title: "Most similar to the papers of your library first" },
    title: { name: "Title" },
    authors: { name: "Authors" },
    journal: { name: "Journal / source" },
    date: { name: "Date", sort: "newest", title: "Newest first" },
    year: { name: "Year" },
    badges: { name: "" },
  };
  let titleAt = $derived(columns.indexOf("title"));
  let lastAt = $derived(columns.length - 1);

  let body = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(0);
  /** the width of the table in view */
  let room = $state(0);
  let now = $state(Date.now());

  /** the widths kept, none under its least */
  let sized = $derived(
    Object.fromEntries(columns.map((c) => [c, Math.max(least(c), widths[c])])) as Record<FeedColumn, number>,
  );
  let template = $derived(
    [`${STAR}px`, `${MARK}px`, ...columns.map((c) => (c === "title" ? `minmax(${sized.title}px, 1fr)` : `${sized[c]}px`))].join(" "),
  );
  let minWidth = $derived(STAR + MARK + columns.reduce((sum, c) => sum + sized[c], 0));
  /** the title wider than its least: it fills the room, the columns after it keep to the right */
  let fills = $derived(room > minWidth);

  let first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - EXTRA));
  let last = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + EXTRA));

  // the ages ("5 min") move on
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 60000);
    return () => clearInterval(timer);
  });

  // the focused row is scrolled into view (moved to with the keys)
  $effect(() => {
    const index = focus ? rows.findIndex((r) => r.id === focus) : -1;
    if (index < 0 || !body) return;
    const top = index * ROW;
    if (top < body.scrollTop) body.scrollTop = top;
    else if (top + ROW > body.scrollTop + body.clientHeight) body.scrollTop = top + ROW - body.clientHeight;
  });

  /** the edge after a column dragged: where it was pressed, the widths drawn then, the title's least then, whether
   *  the title filled the room */
  let resizing: { index: number; x: number; drawn: number[]; titleLeast: number; filled: boolean } | null = null;

  function edgeDown(e: PointerEvent, index: number) {
    if (e.button !== 0) return;
    e.stopPropagation();
    const title = fills ? room - minWidth + sized.title : sized.title;
    const drawn = columns.map((c) => (c === "title" ? title : sized[c]));
    resizing = { index, x: e.clientX, drawn, titleLeast: sized.title, filled: fills };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  /** The edge follows the pointer; the other columns keep their widths, the title takes the room left. Before the
   *  title, or with the title at its least, an edge resizes the column before it; after the title, which fills the
   *  room, the column after it, the title giving the room or taking it (its least going down with it). */
  function edgeMove(e: PointerEvent) {
    if (!resizing) return;
    const { index, x, drawn, titleLeast, filled } = resizing;
    const moved = Math.round(e.clientX - x);
    if (index < titleAt || index === lastAt || !filled) {
      const column = columns[index];
      onwidths({ ...widths, [column]: Math.max(least(column), drawn[index] + moved) });
      return;
    }
    const column = columns[index + 1];
    // as far as the column after the edge and the title allow
    const change = Math.min(drawn[index + 1] - least(column), Math.max(least("title") - drawn[titleAt], moved));
    onwidths({ ...widths, [column]: drawn[index + 1] - change, title: Math.min(titleLeast, drawn[titleAt] + change) });
  }

  /** The family names of a row: three, then "et al." */
  const authorsOf = (row: FeedRow) => row.authors.slice(0, 3);
</script>

<div class="table" role="grid" aria-rowcount={rows.length} bind:clientWidth={room}>
  <div class="head row" role="row" style:grid-template-columns={template} style:min-width="{minWidth}px">
    <span></span>
    <span></span>
    {#each columns as column, i (column)}
      {@const header = HEADERS[column]}
      {@const by = header.sort}
      <div class="head-cell" role="columnheader">
        {#if by && onsort}
          <button class="cell sort" class:sorted={sort === by} onclick={() => onsort(by)} title={header.title}>
            <span class="name">{names[column] ?? header.name}</span>{#if sort === by}<span class="arrow">▾</span>{/if}
          </button>
        {:else}
          <span class="cell"><span class="name">{names[column] ?? header.name}</span></span>
        {/if}
        <!-- the end of the table, while the title fills the room, does not move -->
        {#if i < lastAt || !fills}
          <span
            class="edge"
            role="separator"
            aria-orientation="vertical"
            onpointerdown={(e) => edgeDown(e, i)}
            onpointermove={edgeMove}
            onpointerup={() => (resizing = null)}
          ></span>
        {/if}
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
          class="row {unread ? row.triage : ''}"
          class:selected={selected.has(row.id)}
          role="row"
          tabindex="-1"
          aria-selected={selected.has(row.id)}
          style:top="{(first + i) * ROW}px"
          style:grid-template-columns={template}
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
              class:on={row.triage === "shortlisted"}
              title={row.triage === "shortlisted" ? star.on : star.off}
              onmousedown={(e) => e.stopPropagation()}
              ondblclick={(e) => e.stopPropagation()}
              onclick={() => onstar(row.id)}>{row.triage === "shortlisted" ? "★" : "☆"}</button
            >
          {/if}
          <span class="mark {row.kind}" title={ITEM_KINDS[row.kind].name}>{row.kind === "paper" ? "" : ITEM_KINDS[row.kind].mark}</span>
          {#each columns as column (column)}
            {#if column === "score"}
              <span class="score">{scoreText(row.score)}</span>
            {:else if column === "cited"}
              <span class="cited muted" title="The works that cite it, as OpenAlex counts them">{citedText(row.cited)}</span>
            {:else if column === "similar"}
              <span class="like muted" title="How similar to the papers of your library (0–100)">{likeText(row.like)}</span>
            {:else if column === "title"}
              <span class="title"><Marked spans={row.title} /></span>
            {:else if column === "authors"}
              <span class="muted">
                {#if row.authors.length}
                  {#each authorsOf(row) as name, j}{#if j}, {/if}<Marked spans={name} />{/each}{#if row.authors.length > 3}, et al.{/if}
                {:else if isPost(row.kind)}
                  {ITEM_KINDS[row.kind].name}
                {/if}
              </span>
            {:else if column === "journal"}
              <span class="muted" title={row.sources.join(", ")}>{row.journal ?? row.sources[0] ?? ""}</span>
            {:else if column === "date"}
              <span class="muted date">{ago(row.date, now)}</span>
            {:else if column === "year"}
              <span class="muted date">{yearOf(row.date)}</span>
            {:else}
              <span class="badges">
                {#each badges(row) as b (b.text)}
                  <span class="badge {b.kind}" style:--c={b.kind === "interest" ? colours.get(b.text) : null}>{b.text}</span>
                {/each}
              </span>
            {/if}
          {/each}
        </div>
      {/each}
    </div>
    {@render after?.()}
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
    display: flex;
    align-items: center;
    height: 100%;
    min-width: 0;
    padding: 0 8px;
    border: none;
    background: none;
    color: var(--muted);
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
  }
  .head-cell {
    position: relative;
    height: 100%;
    min-width: 0;
  }
  /* a header keeps to its column: its name cut short first, then its arrow */
  .head-cell > .cell {
    width: 100%;
    overflow: hidden;
  }
  .cell .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
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
  .cell.sorted {
    color: var(--fg);
  }
  .arrow {
    flex: none;
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
  .body .row > span {
    padding: 0 8px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .row.new .title {
    font-weight: 600;
  }
  .row.seen,
  .row.dismissed {
    color: var(--muted);
  }
  .row.dismissed .title {
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
  .score,
  .cited,
  .like,
  .date {
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .body .row > .mark,
  .body .row > .star {
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
  .mark.preprint {
    color: #a16207;
  }
  .mark.news {
    color: #c2410c;
  }
  .mark.blog {
    color: #0e7490;
  }
  .row.selected .mark {
    color: inherit;
  }
  .badges {
    display: flex;
    gap: 3px;
  }
  .badge {
    flex: none;
    padding: 0 5px;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--muted);
    font-size: 10.5px;
    line-height: 15px;
  }
  .badge.lib {
    border-color: #2f9e6e;
    color: #2f9e6e;
  }
  .badge.pre {
    border-color: #a16207;
    color: #a16207;
  }
  .badge.news {
    border-color: #c2410c;
    color: #c2410c;
  }
  .badge.remind {
    border-color: #d9822b;
    color: #d9822b;
  }
  .badge.interest {
    border-color: var(--c, var(--border));
    color: var(--c, var(--muted));
  }
  .row.selected .badge {
    border-color: currentColor;
    color: inherit;
  }
</style>
