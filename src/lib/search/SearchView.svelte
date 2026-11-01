<script lang="ts">
  // The Search tab of the main window: a search of OpenAlex's works, as a search engine of papers, to find papers
  // and add them to the library. The query is the app's (words, "phrases", -word, t:, abs:, a:, j:, y:, doi:, and
  // cites:, citedby: for the works around a work), the Year field added as y:; Enter runs it, a page of 25 works at
  // a time (*Show 25 more* at the end of the list). A new search takes the place of the works found before, but for
  // those saved for later (the star, S) or added. The sidebar: the works found (Results), those saved for later, the
  // recent searches. The list and the details are the Feeds tab's (ItemPane, on an ItemList), with the store
  // "search": the details beside the list or in place of it (the setting `feeds_beside`), as in the Feeds tab. The
  // window (src/routes/library/+page.svelte) keeps the layout (the fields, the recent searches, the search shown),
  // and passes on the commands of the menu and the keys while the tab is in front. The Add sheet is the library's.

  import { listen } from "@tauri-apps/api/event";
  import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
  import { onMount, type Snippet } from "svelte";
  import * as feedsApi from "$lib/feeds/api";
  import type { FeedDetail, FeedRow } from "$lib/feeds/api";
  import { SEARCH_ORDER, SEARCH_SPECS, type FeedColumn } from "$lib/feeds/FeedTable.svelte";
  import type { Shown } from "$lib/list/columns";
  import { interestColours } from "$lib/feeds/filters";
  import { mergeRows, toggled } from "$lib/feeds/items";
  import { modalOpen } from "$lib/library/modal.svelte";
  import Splitter from "$lib/library/Splitter.svelte";
  import { ItemList, typing } from "$lib/list/ItemList.svelte";
  import ItemPane from "$lib/list/ItemPane.svelte";
  import "$lib/list/tab.css";
  import * as api from "./api";
  import {
    ORDERS,
    aroundQuery,
    appended,
    composed,
    moreText,
    totalText,
    withRecent,
    type SearchState,
    type Typed,
  } from "./state";
  import SearchSidebar from "./SearchSidebar.svelte";

  let {
    tabs,
    search = $bindable(),
    columns = $bindable(),
    sidebar = $bindable(),
    details = $bindable(),
    showSidebar,
    showDetails,
    onadd,
    onentry,
  }: {
    /** the switch of the tabs of the window, at the start of the toolbar */
    tabs: Snippet;
    /** the fields, the recent searches and the search shown: the window's layout keeps them */
    search: SearchState;
    /** the list's columns, in their order, with their widths: the window's layout keeps them */
    columns: Shown<FeedColumn>[];
    /** the widths of the side panes and whether they show: the window's, as in the library */
    sidebar: number;
    details: number;
    showSidebar: boolean;
    showDetails: boolean;
    /** the library's Add sheet, over this tab: a DOI (or arXiv id) to fetch, its free PDF looked for or not. The
     *  answer is what was added, said; none when the sheet was closed */
    onadd: (initial: { id: string; pdf: boolean }) => Promise<string | null>;
    /** a paper of the library shown, in the Library tab */
    onentry: (id: string) => void;
  } = $props();

  /** the list shown: the works of the search, or those saved for later */
  let view = $state<"results" | "saved">("results");
  /** the list came, once */
  let loaded = $state(false);
  /** the works saved for later */
  let savedCount = $state(0);
  /** a search asked; the next page asked */
  let searching = $state(false);
  let more = $state(false);
  /** what the search answered instead of works: the query refused, OpenAlex not reached */
  let failure = $state<string | null>(null);
  let following = $state(false);
  /** the colours of the interests, as in the Feeds tab */
  let colours = $state(new Map<string, string>());
  let yearInput = $state<HTMLInputElement>();
  /** the searches asked: an answer to one before the last is dropped */
  let asked = 0;

  /** the works of the search shown, or those saved for later; those unsaved in *Saved for later* stay in place
   *  until another list is shown */
  const list = new ItemList({
    store: "search",
    query: () => marked,
    onadd: (initial) => onadd(initial),
    reload: () => refresh(),
  });
  let results = $derived(search.results);
  /** the query whose words the rows and the details mark */
  let marked = $derived(view === "results" ? (results?.query ?? "") : "");
  let orderText = $derived(ORDERS.find((o) => o.order === search.order)?.text ?? "");
  let summary = $derived(
    (view === "saved"
      ? `${list.rows.length} saved`
      : results
        ? totalText(results.total) + (results.ids.length < results.total ? ` · ${results.ids.length} shown` : "")
        : "") + (list.selection.selected.size > 1 ? ` · ${list.selection.selected.size} selected` : ""),
  );
  /** no table: a search asked or refused, none yet, no works */
  let blank = $derived(
    !loaded || !list.rows.length || (view === "results" && (searching || failure !== null || !results)),
  );

  onMount(() => {
    // a triage, an abstract looked up, the library indexed again
    const unlisten = listen(api.EVENTS.changed, () => {
      list.version++;
      refresh();
    });
    feedsApi.settings().then(
      (s) => (colours = interestColours(s.interests.map((i) => i.name))),
      () => {},
    );
    refresh();
    return () => {
      unlisten.then((f) => f());
    };
  });

  /** The list again as the works are now: those of the search shown, found again, or those saved for later (those
   *  unsaved here kept in place); and how many are saved. An answer for a list no longer shown (another list, another
   *  search or page of it: another `results`) is dropped. */
  async function refresh() {
    const shown = view;
    const r = results;
    try {
      const saved = await api.saved();
      const fresh = shown === "saved" ? saved : r ? await api.rowsOf(r.ids, r.query) : [];
      savedCount = saved.length;
      if (view !== shown || results !== r) return;
      list.rows = shown === "saved" ? mergeRows(list.rows, fresh, list.sticky) : fresh;
    } catch (e) {
      list.say(String(e));
    }
    loaded = true;
    list.selection.prune(list.ids, true);
  }

  /** Another list in place of the one shown: nothing selected, nothing kept in place, the list again. */
  function resetList() {
    list.sticky.clear();
    list.selection.only(null);
    list.reading = false;
  }

  /** Another list, from the sidebar, as it is now. */
  function showList(to: "results" | "saved") {
    view = to;
    resetList();
    refresh();
  }

  /** Enter, Search: the first page of the query of the fields, in place of the works found before (but for those
   *  saved for later or added); kept in the recent searches. A query OpenAlex cannot be asked (years that are none,
   *  a field it has not) is said where the works would be. */
  async function run() {
    const query = composed(search.query, search.year);
    if (!query) return list.focusSearch();
    const typed: Typed = { query: search.query, year: search.year, order: search.order };
    const n = ++asked;
    view = "results";
    resetList();
    searching = true;
    failure = null;
    try {
      const page = await api.run(query, typed.order, null);
      if (n !== asked) return;
      search.results = { query, order: typed.order, ids: page.rows.map((r) => r.id), total: page.total, next: page.next };
      search.recent = withRecent(search.recent, typed);
      list.rows = page.rows;
    } catch (e) {
      if (n === asked) failure = String(e);
    } finally {
      if (n === asked) searching = false;
    }
    refresh();
  }

  /** Show 25 more: the next page of the search shown, after the works shown; the search shown, a new one with it. */
  async function loadMore() {
    const r = results;
    if (!r?.next || more || searching) return;
    more = true;
    try {
      const page = await api.run(r.query, r.order, r.next);
      if (results !== r) return;
      const ids = [...new Set([...r.ids, ...page.rows.map((w) => w.id)])];
      search.results = { ...r, ids, total: page.total, next: page.next };
      if (view === "results") list.rows = appended(list.rows, page.rows);
    } catch (e) {
      list.say(String(e));
    } finally {
      more = false;
    }
  }

  /** A recent search, or the works around a work, in the fields, run. */
  function runAgain(again: Typed) {
    search.query = again.query;
    search.year = again.year;
    search.order = again.order;
    run();
  }

  /** Cited by: the works that cite it (`cites:`); References: those it cites (`citedby:`); the most cited first. */
  function around(d: Pick<FeedDetail, "openalex" | "doi">, facet: "cites" | "citedby") {
    const query = aroundQuery(d, facet);
    if (query) runAgain({ query, year: "", order: "cited" });
  }

  /** The order of the works found, from its menu: the search of the fields again in it. */
  async function orderMenu() {
    const choose = (order: Typed["order"]) => {
      search.order = order;
      if (composed(search.query, search.year) !== "") run();
    };
    const menu = await Menu.new({
      items: await Promise.all(
        ORDERS.map((o) => CheckMenuItem.new({ text: o.text, checked: search.order === o.order, action: () => choose(o.order) })),
      ),
    });
    await menu.popup();
  }

  /** The chip's ×: no search shown, the fields blank (the works found stay until the next search). */
  function clearSearch() {
    search.query = "";
    search.year = "";
    search.results = null;
    list.rows = [];
    failure = null;
    resetList();
    list.focusSearch();
  }

  /** Follow this search: a sweep of OpenAlex in the Feeds tab, fetched at once. */
  async function follow() {
    const query = results?.query;
    if (!query || following) return;
    following = true;
    try {
      const info = await api.follow(query);
      list.say(`Following "${info.title}" in Feeds`);
      feedsApi.fetchFeeds(info.id).catch((e) => list.say(String(e)));
    } catch (e) {
      list.say(String(e));
    } finally {
      following = false;
    }
  }

  /** s, the star: the works saved for later, or no longer when all are (`toggled`); those added keep being added. */
  async function save(ids = list.targets()) {
    const found = await Promise.all(ids.map((id) => list.byId.get(id) ?? list.detailOf(id)));
    const items = found.filter((r): r is FeedRow | FeedDetail => !!r && r.triage !== "added");
    const change = toggled(items, "shortlist");
    if (!change) return;
    const n = change.ids.length === 1 ? "" : ` ${change.ids.length}`;
    list.say(change.triage === "shortlisted" ? `Saved${n} for later` : `No longer saved${n}`);
    await list.setTo(change.ids, change.triage);
  }

  /** Right-click on a row: the works selected when it is one of them, else that work alone. */
  async function rowMenu(id: string) {
    if (!list.selection.selected.has(id)) list.showItem(id);
    const ids = list.targets();
    const items = ids.map((i) => list.byId.get(i)).filter((r): r is FeedRow => r !== undefined);
    if (!items.length) return;
    const savable = items.filter((r) => r.triage !== "added");
    const one = ids.length === 1 ? await list.detailOf(id) : null;
    const knows = !!one && !!(one.openalex ?? one.doi);
    const item = (text: string, action: () => void, enabled = true) => MenuItem.new({ text, action, enabled });
    const separator = () => PredefinedMenuItem.new({ item: "Separator" });
    const menu = await Menu.new({
      items: [
        await item("Open", () => list.openItems(ids)),
        await item("Add…", () => list.addItems(ids, true)),
        await item("Add without PDF…", () => list.addItems(ids, false)),
        await separator(),
        await item(
          savable.length && savable.every((r) => r.triage === "shortlisted") ? "Remove from Saved for Later" : "Save for Later",
          () => save(ids),
          savable.length > 0,
        ),
        await separator(),
        await item("Cited By", () => one && around(one, "cites"), knows),
        await item("References", () => one && around(one, "citedby"), knows),
      ],
    });
    await menu.popup();
  }

  /** ⌘F, /: the search field. */
  export function focusSearch() {
    list.focusSearch();
  }

  /** A key of the window while the tab is in front (none while a sheet is open): Enter in the fields runs the
   *  search; the list's keys move and read (`navKey`); s, a, o act on the selection. */
  export function key(e: KeyboardEvent) {
    if (modalOpen()) return;
    const act = (action: () => unknown) => {
      e.preventDefault();
      action();
    };
    const field = e.target === list.input || e.target === yearInput;
    if (field && e.key === "Enter" && !e.isComposing) return act(run);
    if (list.navKey(e) || typing(e) || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
    switch (e.key) {
      case "s":
        return act(() => save());
      case "a":
        return act(() => list.addItems(list.targets(), true));
      case "o":
        return act(() => list.openItems(list.targets()));
    }
  }

  /** An example of the help, in the search field. */
  function example(text: string) {
    search.query = text;
    search.year = "";
    list.input?.focus();
  }
</script>

<main class="tab" style:--tint="var(--search)">
  {#if showSidebar}
    <aside class="sidebar" style:width="{sidebar}px">
      <SearchSidebar
        {view}
        total={results?.total ?? null}
        saved={savedCount}
        recent={search.recent}
        onview={showList}
        onrecent={runAgain}
        onclear={() => (search.recent = [])}
      />
    </aside>
    <Splitter bind:width={sidebar} side="left" min={160} max={420} />
  {/if}
  <div class="right">
    <header>
      {@render tabs()}
      <div class="search">
        <input
          bind:this={list.input}
          bind:value={search.query}
          type="search"
          placeholder={'Search OpenAlex: words, "phrases", -word, t:, abs:, a:, j:, doi:'}
          spellcheck="false"
          autocomplete="off"
        />
        {#if search.query}
          <button class="clear" aria-label="Clear the search field" onclick={() => ((search.query = ""), list.input?.focus())}>×</button>
        {/if}
      </div>
      <input
        bind:this={yearInput}
        bind:value={search.year}
        class="year"
        placeholder="Year"
        title="The years of the works: 2005, 2000-2010, 2000- or -2010 (y: in the query)"
        spellcheck="false"
        autocomplete="off"
      />
      <button class="tool primary" disabled={searching} title="Search OpenAlex (Enter in the fields)" onclick={run}>
        {searching ? "Searching…" : "Search"}
      </button>
      <span class="notice">{list.notice ?? ""}</span>
      <span class="spacer"></span>
      <button
        class="tool"
        disabled={!results || following}
        title={results
          ? `Follow “${results.query}” in the Feeds tab: its new works come with the feeds (a sweep of OpenAlex)`
          : "Search first, then follow the search in the Feeds tab"}
        onclick={follow}
      >
        {following ? "Following…" : "Follow this search"}
      </button>
    </header>
    <ItemPane
      {list}
      table={{
        columns,
        specs: SEARCH_SPECS,
        menu: SEARCH_ORDER,
        unread: false,
        star: { on: "Saved for later: unsave", off: "Save for later" },
        after: view === "results" && results?.next ? showMore : undefined,
        oncolumns: (shown) => (columns = shown),
        onstar: (id) => save([id]),
        onmenu: rowMenu,
      }}
      {colours}
      tint="var(--search)"
      none="No work selected"
      bind:details
      {showDetails}
      bar={listBar}
      empty={blank ? noRows : undefined}
      actions={itemActions}
      {onentry}
    />
    <footer class="status">
      <span>{searching ? "Searching OpenAlex…" : "OpenAlex's works"}</span>
      <span class="spacer"></span>
      <span>Saved for later {savedCount}</span>
    </footer>
  </div>
</main>

<!-- above the list: the search shown as a chip, the count and the order; the selection bar -->
{#snippet listBar()}
  <div class="filters">
    {#if view === "saved"}
      <span class="chip"><span class="text">★ Saved for later</span></span>
    {:else if results}
      <span class="chip">
        <span class="text" title={results.query}>{results.query}</span>
        <button aria-label="Clear the search" onclick={clearSearch}>×</button>
      </span>
    {/if}
    <span class="spacer"></span>
    <span class="count">{summary}</span>
    {#if view === "results"}
      <button class="sort" title="The order of the works found: the best match, the most cited or the newest first" onclick={orderMenu}>
        {orderText} ▾
      </button>
    {/if}
  </div>
  {#if list.selection.selected.size > 1}
    <div class="selbar">
      <span>{list.selection.selected.size} selected</span>
      <button onclick={() => list.addItems(list.chosen(), true)}>Add…</button>
      <button onclick={() => save(list.chosen())}>☆ Save for later</button>
      <button onclick={() => list.openItems(list.chosen())}>Open ↗</button>
      <span class="spacer"></span>
      <button class="plain" onclick={() => list.selection.only(list.selection.focus)}>Clear selection</button>
    </div>
  {/if}
{/snippet}

<!-- in place of the list: searching, refused, before a search (the help), no works -->
{#snippet noRows()}
  {#if view === "results" && searching}
    <p class="empty"><span class="spin">↻</span> Searching OpenAlex…</p>
  {:else if view === "results" && failure}
    <p class="empty error">{failure}</p>
  {:else if !loaded}
    <p class="empty"></p>
  {:else if view === "results" && !results}
    {@render help()}
  {:else}
    <p class="empty">
      {view === "saved"
        ? "Nothing saved for later. ☆ or S keeps a work found, to come back to it without searching again."
        : "No works found."}
    </p>
  {/if}
{/snippet}

{#snippet showMore()}
  {#if results}
    <div class="more">
      <button disabled={more} onclick={loadMore}>{more ? "Loading…" : moreText(results.ids.length, results.total)}</button>
      <span>{results.ids.length.toLocaleString("en-US")} of {totalText(results.total)}</span>
    </div>
  {/if}
{/snippet}

<!-- the buttons of the details: add, open, save for later, the works around it -->
{#snippet itemActions(d: FeedDetail)}
  <!-- the works around it are searched by its OpenAlex id, else its DOI -->
  {@const unknown = !d.openalex && !d.doi}
  <button class="primary" onclick={() => list.addItems(list.targetsOr(d.id), true)} title="Add to the library, with the free PDF if there is one">Add <kbd>A</kbd></button>
  <button class="open" onclick={() => list.openItems(list.targetsOr(d.id))} disabled={!d.url && !d.doi} title="Open its page">Open ↗ <kbd>O</kbd></button>
  <button onclick={() => list.addItems(list.targetsOr(d.id), false)} title="Add to the library without looking for its PDF">Add without PDF</button>
  {#if d.triage !== "added"}
    <button onclick={() => save(list.targetsOr(d.id))} title="Keep it, to find it again without searching: a new search does not drop it">
      {d.triage === "shortlisted" ? "★ Saved for later" : "☆ Save for later"} <kbd>S</kbd>
    </button>
  {/if}
  <button onclick={() => around(d, "cites")} disabled={unknown} title="Search the works that cite it, the most cited first">Cited by</button>
  <button onclick={() => around(d, "citedby")} disabled={unknown} title="Search the works it cites, the most cited first">References</button>
{/snippet}

<!-- before a search: the query's language, by examples -->
{#snippet help()}
  <div class="help">
    <h3>Search OpenAlex's works</h3>
    <p>
      Words and <code>"phrases"</code> are looked for in the whole work; <code>-word</code> leaves out. The fields:
      <code>t:</code> title, <code>abs:</code> abstract, <code>a:</code> author, <code>j:</code> journal,
      <code>y:</code> years, <code>doi:</code>. The Year field adds <code>y:</code> (2005, 2000-2010, 2000-, -2010).
    </p>
    <ul>
      {#each [["greenland ice sheet y:2000-2010", "words, and the years"], ['t:"surface mass balance" a:Robinson', "a phrase in the title, an author"], ['j:"The Cryosphere" firn', "a word, in a journal"], ["doi:10.5194/tc-", "a work by its DOI (complete it)"]] as [text, what] (text)}
        <li><button class="example" title="Put it in the search field" onclick={() => example(text)}>{text}</button> {what}</li>
      {/each}
    </ul>
    <p>
      On a work, <b>Cited by</b> searches the works that cite it (<code>cites:W…</code>) and <b>References</b> those it
      cites (<code>citedby:W…</code>), the most cited first.
    </p>
    <p>
      ☆ (S) saves a work for later: a new search replaces the works found, but for those saved for later or added.
      <b>Follow this search</b> brings its new works into the Feeds tab.
    </p>
  </div>
{/snippet}

<style>
  /* the years: room for 2000-2010 */
  input.year {
    flex: none;
    width: 92px;
    padding: 0 8px;
    font-variant-numeric: tabular-nums;
  }
  /* Search: in the tab's violet */
  .tool.primary {
    border-color: var(--search);
    background: var(--search);
    color: var(--on-search);
  }
  .spin {
    display: inline-block;
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* the search shown as a chip, the count and the order */
  .filters {
    min-height: 20px;
  }
  .chip {
    max-width: min(480px, 60%);
  }
  .count {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  /* after the last row: the next page */
  .more {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 12px;
    color: var(--muted);
    font-size: 12px;
  }
  .more button {
    padding: 4px 14px;
    border: 1px solid color-mix(in srgb, var(--search) 40%, transparent);
    border-radius: 6px;
    background: color-mix(in srgb, var(--search) 10%, transparent);
    color: var(--fg);
    font: inherit;
  }
  .more button:hover:not(:disabled) {
    background: color-mix(in srgb, var(--search) 20%, transparent);
  }
  .more button:disabled {
    opacity: 0.6;
  }

  /* before a search */
  .help {
    box-sizing: border-box;
    align-self: center;
    width: 100%;
    max-width: 640px;
    min-height: 0;
    padding: 28px 24px;
    overflow-y: auto;
    color: var(--muted);
    line-height: 1.55;
    -webkit-user-select: text;
    user-select: text;
  }
  .help h3 {
    margin: 0 0 10px;
    color: var(--fg);
    font-size: 15px;
  }
  .help p {
    margin: 0 0 10px;
  }
  .help ul {
    margin: 0 0 12px;
    padding: 0;
    list-style: none;
  }
  .help li {
    margin: 0 0 4px;
  }
  .help b {
    color: var(--fg);
    font-weight: 600;
  }
  code,
  .example {
    font: 12px ui-monospace, monospace;
    color: var(--fg);
  }
  .example {
    margin-right: 6px;
    padding: 1px 6px;
    border: 1px solid color-mix(in srgb, var(--search) 35%, transparent);
    border-radius: 5px;
    background: color-mix(in srgb, var(--search) 8%, transparent);
  }
  .example:hover {
    background: color-mix(in srgb, var(--search) 18%, transparent);
  }

</style>
