<script lang="ts">
  // The sidebar of the Search tab: the works of the search shown (Results), those saved for later, the recent
  // searches (a click runs one again; Clear forgets them).

  import { invoke } from "@tauri-apps/api/core";
  import { recentText, totalText, type Asked } from "./query";

  let {
    view,
    total,
    saved,
    recent,
    onview,
    onrecent,
    onclear,
  }: {
    /** the list shown */
    view: "results" | "saved";
    /** the works the search shown found in all; none before a search */
    total: number | null;
    /** the works saved for later */
    saved: number;
    /** the recent searches, the latest first */
    recent: Asked[];
    onview: (view: "results" | "saved") => void;
    /** a recent search clicked: run again */
    onrecent: (search: Asked) => void;
    /** the recent searches forgotten */
    onclear: () => void;
  } = $props();
</script>

<div class="box">
  <nav>
    <h4>Search</h4>
    <button class="item" class:selected={view === "results"} onclick={() => onview("results")}>
      <span class="mark">⌕</span>
      <span class="name">Results</span><span class="count" title={total === null ? null : totalText(total)}>{total === null ? "" : total.toLocaleString("en-US")}</span>
    </button>
    <button
      class="item"
      class:selected={view === "saved"}
      title="The works saved for later: kept when a new search replaces the results"
      onclick={() => onview("saved")}
    >
      <span class="mark star">★</span>
      <span class="name">Saved for later</span><span class="count">{saved}</span>
    </button>

    <h4>
      <span class="heading">
        Recent searches
        {#if recent.length}<button class="clear" title="Forget the recent searches" onclick={onclear}>Clear</button>{/if}
      </span>
    </h4>
    {#each recent as r, i (i)}
      <button class="item" title="{recentText(r)}: search again" onclick={() => onrecent(r)}>
        <span class="name">{recentText(r)}</span>
      </button>
    {:else}
      <p class="none">None yet</p>
    {/each}
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
  /* the sidebar fills its pane: the gear stays at the bottom, as in the library and the Feeds tab */
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
  /* Recent searches: Clear on hover */
  .heading {
    display: flex;
    align-items: center;
  }
  .clear {
    margin-left: auto;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    font-weight: 400;
    opacity: 0;
  }
  h4:hover .clear {
    opacity: 1;
  }
  .clear:hover {
    color: var(--fg);
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
  .item:hover {
    background: color-mix(in srgb, var(--line) 50%, transparent);
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
  .mark {
    flex: none;
    width: 14px;
    font-size: 12px;
    text-align: center;
    color: var(--search);
  }
  .star {
    color: #d9822b;
  }
  .none {
    margin: 2px 16px;
    color: var(--muted);
    font-size: 12px;
  }
</style>
