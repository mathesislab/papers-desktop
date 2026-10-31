<script lang="ts">
  // The list of the Feeds tab: the items the filters select, best score, most like the library or newest first. Only the rows in view are
  // drawn, as in the library's table. A new item's title is bold; one seen is muted. A click selects (⇧ a range,
  // ⌘ one more), a double-click opens the item's page, a right-click its menu.

  import Marked from "$lib/Marked.svelte";
  import type { FeedRow, ItemKind, Sort } from "./api";
  import { ago, badges, likeText, scoreText } from "./items";

  let {
    rows,
    selected,
    focus,
    sort,
    colours,
    onsort,
    onselect,
    onopen,
    onmenu,
  }: {
    rows: FeedRow[];
    selected: Set<string>;
    /** the row the details show, and that the keys move from */
    focus: string | null;
    sort: Sort;
    /** the colours of the interests */
    colours: Map<string, string>;
    onsort: (sort: Sort) => void;
    onselect: (id: string, e: MouseEvent) => void;
    onopen: (id: string) => void;
    /** right-click on a row: its context menu */
    onmenu: (id: string) => void;
  } = $props();

  const ROW = 26;
  /** rows drawn above and below the ones in view, for a smooth scroll */
  const EXTRA = 12;
  const KIND_MARK: Record<ItemKind, string> = { paper: "", preprint: "◐", news: "✦", blog: "✎" };
  const TEMPLATE = "24px 44px 40px minmax(240px, 1fr) 150px 150px 52px 190px";

  let body = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(0);
  let now = $state(Date.now());

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

  /** The family names of a row: three, then "et al." */
  const authorsOf = (row: FeedRow) => row.authors.slice(0, 3);
</script>

<div class="table" role="grid" aria-rowcount={rows.length}>
  <div class="head row" role="row" style:grid-template-columns={TEMPLATE}>
    <span></span>
    <button class="cell sort" class:sorted={sort === "score"} onclick={() => onsort("score")} title="Best score first">
      Score{#if sort === "score"}<span class="arrow">▾</span>{/if}
    </button>
    <button class="cell sort" class:sorted={sort === "like"} onclick={() => onsort("like")} title="Most like the papers of your library first">
      Like{#if sort === "like"}<span class="arrow">▾</span>{/if}
    </button>
    <span class="cell">Title</span>
    <span class="cell">Authors</span>
    <span class="cell">Journal / source</span>
    <button class="cell sort" class:sorted={sort === "newest"} onclick={() => onsort("newest")} title="Newest first">
      Date{#if sort === "newest"}<span class="arrow">▾</span>{/if}
    </button>
    <span class="cell"></span>
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
          style:grid-template-columns={TEMPLATE}
          onmousedown={(e) => onselect(row.id, e)}
          ondblclick={() => onopen(row.id)}
          oncontextmenu={(e) => {
            e.preventDefault();
            onmenu(row.id);
          }}
        >
          <span class="mark {row.kind}" title={row.triage === "shortlisted" ? "Shortlisted" : row.triage === "added" ? "Added" : row.kind}>
            {row.triage === "shortlisted" ? "★" : row.triage === "added" ? "✓" : KIND_MARK[row.kind]}
          </span>
          <span class="score">{scoreText(row.score)}</span>
          <span class="like muted" title="How like the papers of your library (0–100)">{likeText(row.like)}</span>
          <span class="title"><Marked spans={row.title} /></span>
          <span class="muted">
            {#if row.authors.length}
              {#each authorsOf(row) as name, j}{#if j}, {/if}<Marked spans={name} />{/each}{#if row.authors.length > 3}, et al.{/if}
            {:else if row.kind === "news" || row.kind === "blog"}
              {row.kind}
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
    min-width: 900px;
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
  .body .row > .mark {
    padding: 0;
    text-align: center;
    font-size: 11px;
  }
  .row.shortlisted .mark {
    color: #d9822b;
  }
  .row.added .mark {
    color: #2f9e6e;
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
