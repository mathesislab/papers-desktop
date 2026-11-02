<script lang="ts">
  // An item read in place of the list (a tab without the details beside the list: Library, Feeds, Search): over the
  // table, which stays as it was, scrolled where it was; its row pinned at the top, its title and its source, in the
  // tab's colour, a click on it the list again (as Esc); the details under it, in a column of reading width.

  import type { Snippet } from "svelte";

  let {
    title,
    source,
    tint = "var(--feeds)",
    onback,
    children,
  }: {
    /** the title of the item read */
    title: Snippet;
    /** its journal, or where it came from */
    source: string;
    /** the colour of the tab: the pinned row's tint and caret */
    tint?: string;
    /** back to the list */
    onback: () => void;
    /** the details of the item */
    children: Snippet;
  } = $props();
</script>

<div class="reader" style:--tint={tint}>
  <button class="pinned" title="Back to the list (Esc)" onclick={onback}>
    <svg class="caret" viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
      <path d="M1.5 3.5h9L6 9.5z" fill="currentColor" />
    </svg>
    <span class="title">{@render title()}</span>
    <span class="muted">{source}</span>
    <span class="muted keys">↑ ↓ previous, next · Esc the list</span>
  </button>
  <div class="page">
    <div class="column">
      {@render children()}
    </div>
  </div>
</div>

<style>
  .reader {
    position: absolute;
    inset: 0;
    z-index: 2;
    display: flex;
    flex-direction: column;
    background: var(--bg);
  }
  .pinned {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 30px;
    padding: 0 12px;
    border: none;
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--tint) 10%, transparent);
    color: var(--fg);
    font: inherit;
    text-align: left;
  }
  .pinned .caret {
    flex: none;
    display: block;
    color: var(--tint);
  }
  .pinned .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-weight: 600;
  }
  .pinned .muted {
    flex: none;
    max-width: 240px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--muted);
    font-size: 12px;
  }
  .page {
    flex: 1;
    min-height: 0;
    display: flex;
    justify-content: center;
  }
  .column {
    flex: 1;
    max-width: 820px;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
</style>
