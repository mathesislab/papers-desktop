<script lang="ts" module>
  /** a column of the list after its mark, which an edge of its header resizes */
  export type FeedColumn = "score" | "similar" | "title" | "authors" | "journal" | "date" | "badges";

  /** the widths of the columns at first, in pixels; the title's is its least (it takes the room left) */
  export const FEED_COLUMNS: Record<FeedColumn, number> = {
    score: 44,
    similar: 58,
    title: 240,
    authors: 150,
    journal: 150,
    date: 52,
    badges: 190,
  };
</script>

<script lang="ts">
  // The list of the Feeds tab: the items the filters select, best score, most similar to the library or newest
  // first. Only the rows in view are drawn, as in the library's table, and an edge of a header resizes its column
  // (the window's layout keeps the widths). A new item's title is bold; one seen is muted. A click selects (⇧ a
  // range, ⌘ one more), a double-click opens the item's page, a right-click its menu; the star at its start stars
  // the item, or unstars it (✓: added).

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
  const ORDER: FeedColumn[] = ["score", "similar", "title", "authors", "journal", "date", "badges"];

  let body = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(0);
  let now = $state(Date.now());

  let template = $derived(
    [`${STAR}px`, `${MARK}px`, ...ORDER.map((c) => (c === "title" ? `minmax(${widths.title}px, 1fr)` : `${widths[c]}px`))].join(" "),
  );
  let minWidth = $derived(STAR + MARK + ORDER.reduce((sum, c) => sum + widths[c], 0));

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

  /** the edge of a header dragged: its column resized */
  let resizing: { column: FeedColumn; x: number; width: number } | null = null;

  function edgeDown(e: PointerEvent, column: FeedColumn) {
    if (e.button !== 0) return;
    e.stopPropagation();
    resizing = { column, x: e.clientX, width: widths[column] };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function edgeMove(e: PointerEvent) {
    if (!resizing) return;
    const { column, x, width } = resizing;
    onwidths({ ...widths, [column]: Math.max(MIN_WIDTH, Math.round(width + e.clientX - x)) });
  }

  /** The family names of a row: three, then "et al." */
  const authorsOf = (row: FeedRow) => row.authors.slice(0, 3);
</script>

<div class="table" role="grid" aria-rowcount={rows.length}>
  <div class="head row" role="row" style:grid-template-columns={template} style:min-width="{minWidth}px">
    <span></span>
    <span></span>
    {#each ORDER as column (column)}
      <div class="head-cell" role="columnheader">
        {#if column === "score"}
          <button class="cell sort" class:sorted={sort === "score"} onclick={() => onsort("score")} title="Best score first">
            Score{#if sort === "score"}<span class="arrow">▾</span>{/if}
          </button>
        {:else if column === "similar"}
          <button class="cell sort" class:sorted={sort === "like"} onclick={() => onsort("like")} title="Most similar to the papers of your library first">
            Similar{#if sort === "like"}<span class="arrow">▾</span>{/if}
          </button>
        {:else if column === "date"}
          <button class="cell sort" class:sorted={sort === "newest"} onclick={() => onsort("newest")} title="Newest first">
            Date{#if sort === "newest"}<span class="arrow">▾</span>{/if}
          </button>
        {:else}
          <span class="cell">{{ title: "Title", authors: "Authors", journal: "Journal / source", badges: "" }[column]}</span>
        {/if}
        <span
          class="edge"
          role="separator"
          aria-orientation="vertical"
          onpointerdown={(e) => edgeDown(e, column)}
          onpointermove={edgeMove}
          onpointerup={() => (resizing = null)}
        ></span>
      </div>
    {/each}
  </div>
  <div class="body" bind:this={body} bind:clientHeight={height} onscroll={() => (scrollTop = body!.scrollTop)}>
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
          style:min-width="{minWidth}px"
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
  .head-cell > .cell {
    width: 100%;
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
  .badge.interest {
    border-color: var(--c, var(--border));
    color: var(--c, var(--muted));
  }
  .row.selected .badge {
    border-color: currentColor;
    color: inherit;
  }
</style>
