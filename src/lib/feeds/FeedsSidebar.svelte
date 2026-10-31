<script lang="ts">
  // The sidebar of the Feeds tab: the views (the items by their triage), the kinds, the interests, the sources in
  // their groups (folded or not). A click chooses one of a group, a second click lets it go (*All* for the views);
  // ⌘-click (Ctrl elsewhere) adds one to the choices of its group, or takes it out. The choices are filters of the
  // list, a chip per group above it. The counts are over every item not dismissed, whatever the search.

  import { invoke } from "@tauri-apps/api/core";
  import type { Snippet } from "svelte";
  import { modClick } from "$lib/keys";
  import type { FeedCounts, ItemKind } from "./api";
  import { KINDS, SHELVES, interestColours, shelfCounts, type FeedFilters, type Shelf, type SourceGroup } from "./filters";

  let {
    saved,
    filters,
    counts,
    groups,
    onview,
    onkind,
    oninterest,
    onsource,
    onfold,
  }: {
    /** the Saved section, at the top */
    saved?: Snippet;
    /** the choices made */
    filters: FeedFilters;
    counts: FeedCounts | null;
    /** the subscriptions, in their groups */
    groups: SourceGroup[];
    onview: (view: Shelf) => void;
    /** a kind, interest or subscription clicked; with ⌘, to add it to the choices or take it out (`toggle`) */
    onkind: (kind: ItemKind, toggle: boolean) => void;
    oninterest: (name: string, toggle: boolean) => void;
    onsource: (title: string, toggle: boolean) => void;
    /** a group of the sources unfolded or folded */
    onfold: (group: string, open: boolean) => void;
  } = $props();

  const KIND_MARK: Record<ItemKind, string> = { paper: "▤", preprint: "◐", news: "✦", blog: "✎" };

  let shelves = $derived(shelfCounts(counts));
  let colours = $derived(interestColours(Object.keys(counts?.interests ?? {})));
</script>

<div class="box">
  <nav>
    {@render saved?.()}

    <h4>Views</h4>
    {#each SHELVES as shelf (shelf.view)}
      <button class="item" class:selected={filters.view === shelf.view} onclick={() => onview(shelf.view)}>
        {#if shelf.view === "shortlist"}<span class="star">★</span>{/if}
        <span class="name">{shelf.name}</span><span class="count">{shelves[shelf.view]}</span>
      </button>
    {/each}

    <h4>Kinds</h4>
    {#each KINDS as { kind, name } (kind)}
      <button class="item" class:selected={filters.kinds.includes(kind)} onclick={(e) => onkind(kind, modClick(e))}>
        <span class="mark {kind}">{KIND_MARK[kind]}</span>
        <span class="name">{name}</span><span class="count">{counts?.kinds[kind] ?? 0}</span>
      </button>
    {/each}

    {#if colours.size}
      <h4>Interests</h4>
      {#each colours as [name, colour] (name)}
        <button class="item" class:selected={filters.interests.includes(name)} onclick={(e) => oninterest(name, modClick(e))}>
          <span class="dot" style:background={colour}></span>
          <span class="name">{name}</span><span class="count">{counts?.interests[name] ?? 0}</span>
        </button>
      {/each}
    {/if}

    {#if groups.length}
      <h4>Sources</h4>
      {#each groups as g (g.group)}
        {@const open = filters.open.includes(g.group) || g.sources.some((s) => filters.sources.includes(s.title))}
        <button class="item group" aria-expanded={open} onclick={() => onfold(g.group, !open)}>
          <svg class="twisty" class:open viewBox="0 0 8 8" width="8" height="8" aria-hidden="true">
            <path d="M2 1l4 3-4 3z" fill="currentColor" />
          </svg>
          <span class="name">{g.name}</span><span class="count">{g.count}</span>
        </button>
        {#if open}
          {#each g.sources as source (source.title)}
            <button
              class="item sub"
              class:selected={filters.sources.includes(source.title)}
              title={source.title}
              onclick={(e) => onsource(source.title, modClick(e))}
            >
              <span class="name">{source.title}</span><span class="count">{source.count}</span>
            </button>
          {/each}
        {/if}
      {/each}
    {/if}
  </nav>

  <div class="bottom">
    <footer>
      <button class="gear" title="Settings" aria-label="Settings" onclick={() => invoke("open_settings")}>
        <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
        </svg>
      </button>
    </footer>
  </div>
</div>

<style>
  /* the sidebar fills its pane: the gear stays at the bottom, as in the library */
  .box {
    display: flex;
    flex-direction: column;
    min-height: 100%;
  }
  nav {
    flex: 1;
    padding: 6px 0 12px;
  }
  .bottom {
    position: sticky;
    bottom: 0;
    background: linear-gradient(var(--sidebar), var(--sidebar)), var(--bg);
  }
  footer {
    padding: 4px 8px;
    border-top: 1px solid var(--line);
  }
  .gear {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 5px;
    background: none;
    color: var(--muted);
    cursor: default;
  }
  .gear:hover {
    background: var(--line);
    color: var(--fg);
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
    box-sizing: border-box;
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
  .item.group {
    gap: 6px;
  }
  .item.sub {
    padding-left: 28px;
  }
  .twisty {
    flex: none;
    color: var(--muted);
    transition: transform 0.1s;
  }
  .twisty.open {
    transform: rotate(90deg);
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
  .star {
    color: #d9822b;
  }
  .mark {
    flex: none;
    width: 14px;
    font-size: 11px;
    text-align: center;
    color: var(--muted);
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
  .dot {
    flex: none;
    width: 9px;
    height: 9px;
    border-radius: 50%;
  }
</style>
