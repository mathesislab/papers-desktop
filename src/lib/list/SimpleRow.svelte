<script lang="ts">
  // What a row of a simple list shows after its lead (the star, a mark): its date or year, then three lines, its
  // journal or source small (and, quiet, what the tab says of it beside: the badges), its title, its authors small;
  // at its end, quiet, what the tab adds (the tags), as much as fits; under the lead and the date, what the tab
  // shows there (the score), the same place in every row. The row is the list's (FeedTable, the library's Table):
  // its grid (`simpleGridOf`), its clicks and its selection; so are the title and the authors, the words searched
  // marked or not. Each cell has its column, the lead's cells before them (`lead`, their number), so that with
  // `under` the row is two rows: the lead and the date over it, the lines and the end over both.

  import type { Snippet } from "svelte";

  let {
    lead,
    date,
    dateTitle,
    source,
    sources,
    title,
    beside,
    authors,
    end,
    under,
  }: {
    /** the number of the cells of the row before these: its star, its mark */
    lead: number;
    /** the date of the row, as an age or a year */
    date: string;
    /** what the date says on hover: its reminder */
    dateTitle?: string;
    /** its journal, or where it came from */
    source: string;
    /** what the source says on hover: where else it came from */
    sources?: string;
    title: Snippet;
    /** after the source, on its line: the badges */
    beside?: Snippet;
    authors: Snippet;
    /** at the end of the row: the tags */
    end?: Snippet;
    /** under the lead and the date: the score */
    under?: Snippet;
  } = $props();
</script>

<span class="muted date" style:grid-column={lead + 1} title={dateTitle}>{date}</span>
<span class="lines" class:both={!!under} style:grid-column={lead + 2}>
  <span class="small sourced"><span class="muted" title={sources}>{source}</span>{@render beside?.()}</span>
  <span>{@render title()}</span>
  <span class="muted small">{@render authors()}</span>
</span>
<span class="muted end" class:both={!!under} style:grid-column={lead + 3}>{@render end?.()}</span>
{#if under}
  <span class="under" style:grid-column="1 / span {lead + 1}">{@render under()}</span>
{/if}

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
    grid-row: 1;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  /* with something under the date: over the two rows */
  .both {
    grid-row: 1 / span 2;
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
  /* the source cut short before what is beside it */
  .lines > .sourced {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .sourced > .muted {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* the end quiet: muted, smaller, as much as fits */
  .end {
    max-width: 220px;
    font-size: 11px;
  }
  .under {
    grid-row: 2;
    align-self: start;
    padding: 0 8px 0 6px;
  }
  .muted {
    color: var(--muted);
  }
  :global(.row.selected) .muted {
    color: inherit;
  }
</style>
