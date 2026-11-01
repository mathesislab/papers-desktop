<script lang="ts">
  // The list of the Feeds tab or the Search tab and its item in focus (an `ItemList`): the table, the item read in
  // place of it, its row pinned at the top (Reader), or its details beside the list (the window's switch), a splitter
  // between, at the window's width. The tab gives what is above the list (`bar`: its filters, the selection bar), what shows in
  // place of the table (`empty`: the list loading, failed or empty), the buttons of the details (`actions`) and the
  // table's columns.

  import type { ComponentProps, Snippet } from "svelte";
  import type { FeedDetail, Linked } from "$lib/feeds/api";
  import FeedDetails from "$lib/feeds/FeedDetails.svelte";
  import FeedTable from "$lib/feeds/FeedTable.svelte";
  import Reader from "$lib/feeds/Reader.svelte";
  import Splitter from "$lib/library/Splitter.svelte";
  import type { ItemList } from "./ItemList.svelte";
  import { tabContext } from "./tab";

  let {
    list,
    table,
    colours,
    tint,
    none,
    bar,
    empty,
    actions,
    finding = false,
    onaddlink,
    onshortlistlink,
    onfind,
  }: {
    list: ItemList;
    /** the table's own: its columns, their names and widths, the order, the star, what follows the last row */
    table: Omit<ComponentProps<typeof FeedTable>, "rows" | "selected" | "focus" | "colours" | "onselect" | "onopen">;
    /** the colours of the interests */
    colours: Map<string, string>;
    /** the colour of the tab: the row pinned over the item read */
    tint?: string;
    /** the details beside the list, with no item in focus */
    none: string;
    /** above the list */
    bar: Snippet;
    /** in place of the table; none: the table */
    empty?: Snippet;
    /** the buttons of the details, for the item shown */
    actions: Snippet<[FeedDetail]>;
    /** a post's papers (the Feeds tab's): being looked for; one added, one starred, looked for again */
    finding?: boolean;
    onaddlink?: (link: Linked) => void;
    onshortlistlink?: (item: string) => void;
    onfind?: (id: string) => void;
  } = $props();

  const tab = tabContext();

  let selection = $derived(list.selection);
</script>

<div class="content">
  <section class="list">
    {@render bar()}
    {#if empty}
      {@render empty()}
    {:else}
      <div class="rows">
        <FeedTable
          {...table}
          rows={list.rows}
          selected={selection.selected}
          focus={selection.focus}
          {colours}
          onselect={list.select}
          onopen={(id) => list.openItems(selection.selected.has(id) ? list.targets() : [id])}
        />
        {#if list.reading && !list.beside && list.focusRow}
          <Reader row={list.focusRow} {tint} onback={() => (list.reading = false)}>
            {#if list.detail}{@render itemDetails(list.detail, true)}{/if}
          </Reader>
        {/if}
      </div>
    {/if}
  </section>
  {#if list.beside && tab.showDetails}
    <Splitter bind:width={tab.details} side="right" min={260} max={600} />
    <aside class="details" style:width="{tab.details}px">
      {#if list.detail}
        {@render itemDetails(list.detail, false)}
      {:else}
        <p class="none">{none}</p>
      {/if}
    </aside>
  {/if}
</div>

{#snippet itemDetails(d: FeedDetail, wide: boolean)}
  <FeedDetails
    detail={d}
    {wide}
    count={selection.focus !== null && selection.selected.has(selection.focus) ? selection.selected.size : 1}
    byId={list.byId}
    {colours}
    store={list.store}
    {actions}
    {finding}
    {onaddlink}
    {onshortlistlink}
    onfind={onfind && (() => onfind(d.id))}
    onitem={list.showItem}
    onentry={tab.onentry}
    onback={wide ? () => (list.reading = false) : undefined}
  />
{/snippet}

<style>
  .content {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .list {
    flex: 1;
    min-width: 320px;
    display: flex;
    flex-direction: column;
  }
  .details {
    flex: none;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--line);
  }
  /* the table, and the item read over it: the table stays as it was, scrolled where it was */
  .rows {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .none {
    margin: auto;
    padding: 24px;
    color: var(--muted);
    text-align: center;
  }
</style>
