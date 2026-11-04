<script lang="ts">
  // The sidebar of the Papers tab: the papers open, the one in front marked, each closed by its ×. A click shows
  // one; the list keeps the order they were opened in.
  import type { Row } from "$lib/types";

  let {
    papers,
    active,
    dirty,
    onpaper,
    onclose,
  }: {
    /** the papers open, in the order they were opened */
    papers: Row[];
    /** the paper in front */
    active: string | null;
    /** the paper whose BibTeX has unsaved changes */
    dirty: string | null;
    onpaper: (id: string) => void;
    onclose: (id: string) => void;
  } = $props();
</script>

<nav>
  <h4>Open</h4>
  {#each papers as paper (paper.id)}
    <div class="paper" class:selected={paper.id === active}>
      <button class="tab" title={paper.title} onclick={() => onpaper(paper.id)}>
        <span class="name">{paper.authors} {paper.year ?? ""}</span>
        <span class="title">{paper.title}</span>
      </button>
      {#if paper.id === dirty}<span class="dot" title="Unsaved changes"></span>{/if}
      <button class="close" aria-label="Close" title="Close the paper" onclick={() => onclose(paper.id)}>×</button>
    </div>
  {:else}
    <p class="none">No paper open</p>
  {/each}
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
  .none {
    margin: 4px 16px;
    color: var(--muted);
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
  .tab .name,
  .tab .title {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .tab .title {
    color: var(--muted);
    font-size: 11.5px;
  }
  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
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
</style>
