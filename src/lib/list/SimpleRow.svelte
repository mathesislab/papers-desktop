<script lang="ts">
  // What a row of a simple list shows after its lead (the star, a mark): its date or year, then three lines, its
  // journal or source small, its title, its authors small; at its end, quiet, what the tab adds (the badges, the
  // tags), as much as fits. The row is the list's (FeedTable, the library's Table): its grid (`simpleGridOf`), its
  // clicks and its selection; so are the title and the authors, the words searched marked or not.

  import type { Snippet } from "svelte";

  let {
    date,
    source,
    sources,
    title,
    authors,
    end,
  }: {
    /** the date of the row, as an age or a year */
    date: string;
    /** its journal, or where it came from */
    source: string;
    /** what the source says on hover: where else it came from */
    sources?: string;
    title: Snippet;
    authors: Snippet;
    /** at the end of the row: the badges, the tags */
    end?: Snippet;
  } = $props();
</script>

<span class="muted date">{date}</span>
<span class="lines">
  <span class="muted small" title={sources}>{source}</span>
  <span>{@render title()}</span>
  <span class="muted small">{@render authors()}</span>
</span>
<span class="end">{@render end?.()}</span>

<style>
  .date,
  .lines,
  .end {
    padding: 0 8px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .date {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .lines {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .lines > span {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    line-height: 16px;
  }
  .lines > .small {
    font-size: 11px;
    line-height: 14px;
  }
  /* the end quiet: smaller, as much as fits */
  .end {
    max-width: 220px;
    font-size: 11px;
  }
  .muted {
    color: var(--muted);
  }
  :global(.row.selected) .muted {
    color: inherit;
  }
</style>
