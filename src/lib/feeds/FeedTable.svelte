<script lang="ts" module>
  /** a column of the list after its mark, which an edge of its header resizes */
  export type FeedColumn = "score" | "similar" | "title" | "authors" | "journal" | "date" | "badges";

  /** the widths of the columns at first, in pixels; the title's is its least (it takes the room left) */
  export const FEED_COLUMNS: Record<FeedColumn, number> = {
    score: 60,
    similar: 70,
    title: 240,
    authors: 150,
    journal: 150,
    date: 58,
    badges: 190,
  };
</script>

<script lang="ts">
  // The list of the Feeds tab: the items the filters select, best score, most similar to the library or newest
  // first. Only the rows in view are drawn, as in the library's table, and an edge of a header resizes a column, the
  // title taking the room left (the window's layout keeps the widths). A new item's title is bold; one seen is
  // muted. A click selects (⇧ a range, ⌘ one more), a double-click opens the item's page, a right-click its menu;
  // the star at its start stars the item, or unstars it (✓: added).

  import Marked from "$lib/Marked.svelte";
  import type { FeedRow, ItemKind, Sort } from "./api";
  import { ago, badges, likeText, scoreText } from "./items";

  let {
    rows,
    selected,
    focus,
    sort,
    widths,
    colours,
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
    sort: Sort;
    widths: Record<FeedColumn, number>;
    /** the colours of the interests */
    colours: Map<string, string>;
    onsort: (sort: Sort) => void;
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
  const KIND_MARK: Record<ItemKind, string> = { paper: "", preprint: "◐", news: "✦", blog: "✎" };
  const STAR = 22;
  const MARK = 18;
  const MIN_WIDTH = 30;
  /** the least widths above MIN_WIDTH: a sorted header and its arrow fit, a title stays readable */
  const LEAST: Partial<Record<FeedColumn, number>> = { score: 60, similar: 70, date: 58, title: 100 };
  const least = (column: FeedColumn) => LEAST[column] ?? MIN_WIDTH;
  const ORDER: FeedColumn[] = ["score", "similar", "title", "authors", "journal", "date", "badges"];
  const TITLE = ORDER.indexOf("title");
  const LAST = ORDER.length - 1;

  let body = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(0);
  /** the width of the table in view */
  let room = $state(0);
  let now = $state(Date.now());

  /** the widths kept, none under its least */
  let sized = $derived(
    Object.fromEntries(ORDER.map((c) => [c, Math.max(least(c), widths[c])])) as Record<FeedColumn, number>,
  );
  let template = $derived(
    [`${STAR}px`, `${MARK}px`, ...ORDER.map((c) => (c === "title" ? `minmax(${sized.title}px, 1fr)` : `${sized[c]}px`))].join(" "),
  );
  let minWidth = $derived(STAR + MARK + ORDER.reduce((sum, c) => sum + sized[c], 0));
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
    const drawn = ORDER.map((c) => (c === "title" ? title : sized[c]));
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
    if (index < TITLE || index === LAST || !filled) {
      const column = ORDER[index];
      onwidths({ ...widths, [column]: Math.max(least(column), drawn[index] + moved) });
      return;
    }
    const column = ORDER[index + 1];
    // as far as the column after the edge and the title allow
    const change = Math.min(drawn[index + 1] - least(column), Math.max(least("title") - drawn[TITLE], moved));
    onwidths({ ...widths, [column]: drawn[index + 1] - change, title: Math.min(titleLeast, drawn[TITLE] + change) });
  }

  /** The family names of a row: three, then "et al." */
  const authorsOf = (row: FeedRow) => row.authors.slice(0, 3);
</script>

<div class="table" role="grid" aria-rowcount={rows.length} bind:clientWidth={room}>
  <div class="head row" role="row" style:grid-template-columns={template} style:min-width="{minWidth}px">
    <span></span>
    <span></span>
    {#each ORDER as column, i (column)}
      <div class="head-cell" role="columnheader">
        {#if column === "score"}
          <button class="cell sort" class:sorted={sort === "score"} onclick={() => onsort("score")} title="Best score first">
            <span class="name">Score</span>{#if sort === "score"}<span class="arrow">▾</span>{/if}
          </button>
        {:else if column === "similar"}
          <button class="cell sort" class:sorted={sort === "like"} onclick={() => onsort("like")} title="Most similar to the papers of your library first">
            <span class="name">Similar</span>{#if sort === "like"}<span class="arrow">▾</span>{/if}
          </button>
        {:else if column === "date"}
          <button class="cell sort" class:sorted={sort === "newest"} onclick={() => onsort("newest")} title="Newest first">
            <span class="name">Date</span>{#if sort === "newest"}<span class="arrow">▾</span>{/if}
          </button>
        {:else}
          <span class="cell"><span class="name">{{ title: "Title", authors: "Authors", journal: "Journal / source", badges: "" }[column]}</span></span>
        {/if}
        <!-- the end of the table, while the title fills the room, does not move -->
        {#if i < LAST || !fills}
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
          class="row {row.triage}"
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
              title={row.triage === "shortlisted" ? "Starred: unstar" : "Star"}
              onmousedown={(e) => e.stopPropagation()}
              ondblclick={(e) => e.stopPropagation()}
              onclick={() => onstar(row.id)}>{row.triage === "shortlisted" ? "★" : "☆"}</button
            >
          {/if}
          <span class="mark {row.kind}" title={row.kind}>{KIND_MARK[row.kind]}</span>
          <span class="score">{scoreText(row.score)}</span>
          <span class="like muted" title="How similar to the papers of your library (0–100)">{likeText(row.like)}</span>
          <span class="title"><Marked spans={row.title} /></span>
          <span class="muted">
            {#if row.authors.length}
              {#each authorsOf(row) as name, j}{#if j}, {/if}<Marked spans={name} />{/each}{#if row.authors.length > 3}, et al.{/if}
            {:else if row.kind === "news" || row.kind === "blog"}
              {row.kind === "news" ? "news" : "general"}
            {/if}
          </span>
          <span class="muted" title={row.sources.join(", ")}>{row.journal ?? row.sources[0] ?? ""}</span>
          <span class="muted date">{ago(row.date, now)}</span>
          <span class="badges">
            {#each badges(row) as b (b.text)}
              <span class="badge {b.kind}" style:--c={b.kind === "interest" ? colours.get(b.text) : null}>{b.text}</span>
            {/each}
          </span>
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
