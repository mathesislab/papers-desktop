<script lang="ts">
  // The Feeds tab of the main window (docs/design.md, Feeds; the mockup docs/mockups/feeds.html): the sidebar (the
  // views, kinds, interests and sources), the list of the items, the details of the one selected. The panes for
  // now: there are no subscriptions yet.

  import type { Snippet } from "svelte";
  import Splitter from "$lib/library/Splitter.svelte";

  let {
    tabs,
    sidebar = $bindable(),
    details = $bindable(),
    showSidebar,
    showDetails,
  }: {
    /** the switch of the tabs of the window, at the start of the toolbar */
    tabs: Snippet;
    /** the widths of the side panes and whether they show: the window's, as in the library */
    sidebar: number;
    details: number;
    showSidebar: boolean;
    showDetails: boolean;
  } = $props();
</script>

<main>
  {#if showSidebar}
    <aside class="sidebar" style:width="{sidebar}px">
      <h4>Subscriptions</h4>
      <p class="none">None yet</p>
    </aside>
    <Splitter bind:width={sidebar} side="left" min={160} max={420} />
  {/if}
  <div class="right">
    <header>
      {@render tabs()}
    </header>
    <div class="content">
      <section class="list">
        <p class="empty">No subscriptions yet</p>
      </section>
      {#if showDetails}
        <Splitter bind:width={details} side="right" min={260} max={600} />
        <aside class="details" style:width="{details}px">
          <p class="empty">No item selected</p>
        </aside>
      {/if}
    </div>
  </div>
</main>

<style>
  main {
    display: flex;
    height: 100vh;
  }

  .sidebar {
    flex: none;
    overflow-y: auto;
    padding: 6px 0 12px;
    box-sizing: border-box;
    background: var(--sidebar);
    border-right: 1px solid var(--line);
  }
  h4 {
    margin: 14px 16px 4px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }
  .none {
    margin: 0;
    padding: 3px 16px;
    color: var(--muted);
  }

  .right {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  header {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 46px;
    padding: 0 12px;
    border-bottom: 1px solid var(--line);
  }
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

  .empty {
    margin: auto;
    padding: 24px;
    color: var(--muted);
    text-align: center;
  }
</style>
