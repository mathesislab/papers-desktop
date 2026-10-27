<script lang="ts" module>
  /** what the table lists: all papers, a view of them, or one .bib file */
  export type View = "all" | "recent" | "nopdf" | "broken" | "duplicates" | { bib: string };

  export const sameView = (a: View, b: View) =>
    typeof a === "string" || typeof b === "string" ? a === b : a.bib === b.bib;
</script>

<script lang="ts">
  import type { TagCount } from "$lib/types";

  let {
    libraries,
    views,
    tags,
    view,
    filtered,
    onview,
    ontag,
  }: {
    /** the .bib files, with their number of papers */
    libraries: { bib: string; count: number }[];
    views: { view: View; name: string; count: number }[];
    tags: TagCount[];
    view: View;
    /** whether the search filters a tag */
    filtered: (tag: string) => boolean;
    onview: (view: View) => void;
    ontag: (tag: string) => void;
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
  .count {
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
</style>
