<script lang="ts" module>
  /** what the table lists: all papers, a view of them, or one .bib file */
  export type View = "all" | "recent" | "nopdf" | "broken" | "duplicates" | { bib: string };

  /** whether the library shows view `b`: not while a paper is in front */
  export const sameView = (a: View | null, b: View) =>
    a !== null && (typeof a === "string" || typeof b === "string" ? a === b : a.bib === b.bib);
</script>

<script lang="ts">
  import type { Row, TagCount } from "$lib/types";

  let {
    libraries,
    views,
    tags,
    view,
    filtered,
    papers,
    active,
    onview,
    ontag,
    onpaper,
    onclose,
  }: {
    /** the .bib files, with their number of papers */
    libraries: { bib: string; count: number }[];
    views: { view: View; name: string; count: number }[];
    tags: TagCount[];
    /** the view of the library; none while a paper is in front */
    view: View | null;
    /** whether the search filters a tag */
    filtered: (tag: string) => boolean;
    /** the papers open, as tabs */
    papers: Row[];
    /** the paper in front */
    active: string | null;
    onview: (view: View) => void;
    ontag: (tag: string) => void;
    onpaper: (id: string) => void;
    onclose: (id: string) => void;
  } = $props();

  const fileName = (path: string) => path.split(/[\\/]/).pop();
</script>

<nav>
  {#if libraries.length > 1}
    <h4>Libraries</h4>
    {#each libraries as { bib, count }}
      <button class="item" class:selected={sameView(view, { bib })} title={bib} onclick={() => onview({ bib })}>
        <span class="name">{fileName(bib)}</span><span class="count">{count}</span>
      </button>
    {/each}
  {/if}

  <h4>Library</h4>
  {#each views as item}
    <button class="item" class:selected={sameView(view, item.view)} onclick={() => onview(item.view)}>
      <span class="name">{item.name}</span><span class="count">{item.count}</span>
    </button>
  {/each}

  {#if tags.length}
    <h4>Tags</h4>
    {#each tags as { tag, count }}
      <button class="item" class:selected={filtered(tag)} onclick={() => ontag(tag)}>
        <span class="name">{tag}</span><span class="count">{count}</span>
      </button>
    {/each}
  {/if}

  {#if papers.length}
    <h4>Open papers</h4>
    {#each papers as paper (paper.id)}
      <div class="paper" class:selected={paper.id === active}>
        <button class="tab" title={paper.title} onclick={() => onpaper(paper.id)}>
          <span class="name">{paper.authors} {paper.year ?? ""}</span>
          <span class="title">{paper.title}</span>
        </button>
        <button class="close" aria-label="Close" onclick={() => onclose(paper.id)}>×</button>
      </div>
    {/each}
  {/if}
</nav>

<style>
  nav {
    padding: 6px 0 12px;
  }
  h4 {
    margin: 14px 16px 4px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: calc(100% - 12px);
    margin: 0 6px;
    padding: 4px 10px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: default;
  }
  .item.selected {
    background: var(--line);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .paper {
    display: flex;
    align-items: center;
    margin: 0 6px;
    border-radius: 6px;
  }
  .paper.selected {
    background: var(--line);
  }
  .tab {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: 4px 4px 4px 10px;
    border: none;
    background: none;
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: default;
  }
  .tab .title {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--muted);
    font-size: 11.5px;
  }
  .close {
    flex: none;
    width: 22px;
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
    font-size: 15px;
    visibility: hidden;
  }
  .paper:hover .close,
  .paper.selected .close {
    visibility: visible;
  }
  .count {
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
</style>
