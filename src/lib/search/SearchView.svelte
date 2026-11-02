<script lang="ts">
  // The Search tab of the main window: a search of OpenAlex's works, as a search engine of papers, to find papers
  // and add them to the library. The query is the app's (words, "phrases", -word, t:, abs:, a:, j:, y:, doi:, and
  // cites:, citedby: for the works around a work), the Author(s) field added as a:, the Year(s) field as y: (on a
  // line of their own, under the search field: OpenAlex matches words in the texts only); Enter runs it, a page of
  // 25 works at a time (*Show 25 more* at the end of the list), the papers of the library closest to it among the
  // first (✓, each work once), all by score at first. A new search takes the place of the works found before, but for
  // those saved for later (☆, S: starred in its folder) or added. The sidebar: the works found (Results), those saved
  // for later, the recent searches. The list and the details are the Feeds tab's (ItemPane, on an ItemList), with the
  // store "search": the details beside the list or in place of it, its actions those of the Feeds tab's table
  // (`itemActions`, the star in the tab's words: `SAVED_FOR_LATER`). The window
  // (src/routes/main/+page.svelte) keeps the layout (the fields, the recent searches, the search shown), gives
  // what both its Feeds and Search tabs take (`tabContext`), and passes on the commands of the menu and the keys
  // while the tab is in front. The Add sheet is the library's. *Search online* opens the query in the browser at a
  // service without an API (Google Scholar, Google, the user's of the settings).

  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import * as feedsApi from "$lib/feeds/api";
  import { buttonText, itemActions, type ItemActionName } from "$lib/feeds/actions";
  import type { FeedDetail } from "$lib/feeds/api";
  import { SEARCH_ORDER, SEARCH_SPECS } from "$lib/feeds/columns";
  import { mergeRows, type TriageAction } from "$lib/feeds/items";
  import { modalOpen } from "$lib/library/modal.svelte";
  import Splitter from "$lib/library/Splitter.svelte";
  import ActionButtons from "$lib/list/ActionButtons.svelte";
  import { ItemList, menuItem, typing } from "$lib/list/ItemList.svelte";
  import ItemPane from "$lib/list/ItemPane.svelte";
  import LookSwitch from "$lib/list/LookSwitch.svelte";
  import type { SearchLayout } from "$lib/layout";
  import "$lib/list/tab.css";
  import { ownedOf, tabContext } from "$lib/list/tab";
  import * as api from "./api";
  import {
    ORDERS,
    aroundQuery,
    askedOf,
    appended,
    inOrder,
    composed,
    plainWords,
    moreText,
    totalText,
    withRecent,
    SAVED_FOR_LATER,
    type Typed,
  } from "./state";
  import SearchSidebar from "./SearchSidebar.svelte";

  let {
    search = $bindable(),
  }: {
    /** the fields, the recent searches, the search shown, the list's columns and look: the window's layout keeps
     *  them */
    search: SearchLayout;
  } = $props();

  const tab = tabContext();

  /** the list shown: the works of the search, or those starred */
  let view = $state<"results" | "starred">("results");
  /** the list came, once */
  let loaded = $state(false);
  /** the works starred */
  let starredCount = $state(0);
  /** a search asked; the next page asked */
  let searching = $state(false);
  let more = $state(false);
  /** what the search answered instead of works: the query refused, OpenAlex not reached */
  let failure = $state<string | null>(null);
  let subscribing = $state(false);
  let yearInput = $state<HTMLInputElement>();
  let authorsInput = $state<HTMLInputElement>();
  let header = $state<HTMLElement>();
  let searchBox = $state<HTMLElement>();
  /** where the search field starts in the header: the Author(s) field starts there too */
  let fieldsLeft = $state(0);

  // the tabs' width changes with the tabs shown, the header's with the window
  $effect(() => {
    if (!header || !searchBox) return;
    const measure = () => (fieldsLeft = searchBox!.getBoundingClientRect().left - header!.getBoundingClientRect().left - 12);
    const observer = new ResizeObserver(measure);
    observer.observe(header);
    measure();
    return () => observer.disconnect();
  });
  /** the searches asked: an answer to one before the last is dropped */
  let asked = 0;

  /** the works of the search shown, or those saved for later; those unsaved in *Saved for later* stay in place
   *  until another list is shown */
  const list = new ItemList({
    store: "search",
    query: () => marked,
    reload: () => refresh(),
    look: () => search.look,
  });
  let results = $derived(search.results);
  /** the query whose words the rows and the details mark */
  let marked = $derived(view === "results" ? (results?.query ?? "") : "");
  let orderText = $derived(ORDERS.find((o) => o.order === search.order)?.text ?? "");
  let summary = $derived(
    (view === "starred"
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
      const starred = await api.starred();
      const fresh = shown === "starred" ? starred : r ? await api.rowsOf(r.ids, r.query) : [];
      starredCount = starred.length;
      if (view !== shown || results !== r) return;
      list.rows = shown === "starred" ? mergeRows(list.rows, fresh, list.sticky) : fresh;
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
  function showList(to: "results" | "starred") {
    view = to;
    resetList();
    refresh();
  }

  /** Enter, Search: the first page of the query of the fields, in place of the works found before (but for those
   *  saved for later or added); kept in the recent searches. A query OpenAlex cannot be asked (years that are none,
   *  a field it has not) is said where the works would be. */
  async function run() {
    const query = composed(search);
    if (!query) return list.focusSearch();
    const typed: Typed = { query: search.query, authors: search.authors, year: search.year, order: search.order };
    const n = ++asked;
    view = "results";
    resetList();
    searching = true;
    failure = null;
    try {
      const page = await api.run(query, plainWords(typed), askedOf(typed.order), null);
      if (n !== asked) return;
      const rows = inOrder(page.rows, typed.order);
      search.results = { query, order: typed.order, ids: rows.map((r) => r.id), total: page.total, next: page.next };
      search.recent = withRecent(search.recent, typed);
      list.rows = rows;
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
      const page = await api.run(r.query, "", askedOf(r.order), r.next);
      if (results !== r) return;
      const rows = inOrder(appended(list.rows, page.rows), r.order);
      search.results = { ...r, ids: rows.map((w) => w.id), total: page.total, next: page.next };
      if (view === "results") list.rows = rows;
    } catch (e) {
      list.say(String(e));
    } finally {
      more = false;
    }
  }

  /** A recent search, or the works around a work, in the fields, run. */
  function runAgain(again: Typed) {
    search.query = again.query;
    search.authors = again.authors;
    search.year = again.year;
    search.order = again.order;
    run();
  }

  /** Cited by: the works that cite it (`cites:`); References: those it cites (`citedby:`); the most cited first. */
  function around(d: Pick<FeedDetail, "openalex" | "doi">, facet: "cites" | "citedby") {
    const query = aroundQuery(d, facet);
    if (query) runAgain({ query, authors: "", year: "", order: "cited" });
  }

  /** The order of the works found, from its menu: the search of the fields again in it. */
  async function orderMenu() {
    const choose = (order: Typed["order"]) => {
      search.order = order;
      if (composed(search) !== "") run();
    };
    const menu = await Menu.new({
      items: await Promise.all(
        ORDERS.map((o) => CheckMenuItem.new({ text: o.text, checked: search.order === o.order, action: () => choose(o.order) })),
      ),
    });
    await menu.popup();
  }

  /** The services of *Search online*, from its menu: the query of the fields opened in the browser at one; *Add a
   *  service…* the settings, where they are listed. */
  async function onlineMenu() {
    const open = async (service: string) => {
      try {
        await openUrl(await api.online(service, composed(search)));
      } catch (e) {
        list.say(String(e));
      }
    };
    const names = await api.services();
    const menu = await Menu.new({
      items: [
        ...(await Promise.all(names.map((name) => MenuItem.new({ text: name, action: () => open(name) })))),
        await PredefinedMenuItem.new({ item: "Separator" }),
        await MenuItem.new({ text: "Add a service…", action: () => invoke("open_settings", { section: "search" }) }),
      ],
    });
    await menu.popup();
  }

  /** The chip's ×: no search shown, the fields blank (the works found stay until the next search). */
  function clearSearch() {
    search.query = "";
    search.authors = "";
    search.year = "";
    search.results = null;
    list.rows = [];
    failure = null;
    resetList();
    list.focusSearch();
  }

  /** Subscribe to this search: a sweep of OpenAlex in the Feeds tab, fetched at once. */
  async function subscribe() {
    const query = results?.query;
    if (!query || subscribing) return;
    subscribing = true;
    try {
      const info = await api.subscribe(query);
      list.say(`Subscribed to "${info.title}" in Feeds`);
      feedsApi.fetchFeeds(info.id).catch((e) => list.say(String(e)));
    } catch (e) {
      list.say(String(e));
    } finally {
      subscribing = false;
    }
  }

  /** s, the star: the works saved for later, or no longer when all are (`ItemList.toggled`); those added keep being
   *  added. */
  async function triage(action: TriageAction, ids: string[]) {
    const change = await list.toggled(action, ids);
    if (!change) return;
    const n = change.ids.length === 1 ? "" : ` ${change.ids.length}`;
    list.say(change.triage === "starred" ? `Saved${n} for later` : `No longer saved${n}`);
    await list.setTo(change.ids, change.triage);
  }

  /** what can be done to the works, as in the Feeds tab: the keys, the menu, the buttons */
  const actions = itemActions(
    {
      open: (ids) => list.openItems(ids),
      add: (ids, pdf) => list.addItems(ids, pdf),
      triage,
    },
    SAVED_FOR_LATER,
  );
  /** the buttons of the details, but for the works around it; those of the selection bar */
  const BUTTONS: ItemActionName[] = ["add", "open", "star"];
  const BAR_BUTTONS: ItemActionName[] = ["add", "star", "open"];

  /** Right-click on a row: the works selected when it is one of them, else that work alone. */
  async function rowMenu(id: string) {
    if (!list.selection.selected.has(id)) list.showItem(id);
    const ids = list.targets();
    const items = list.itemsOf(ids);
    if (!items.length) return;
    const one = ids.length === 1 ? await list.detailOf(id) : null;
    const knows = !!one && !!(one.openalex ?? one.doi);
    const item = (name: ItemActionName) => menuItem(actions[name], ids, items);
    const aroundItem = (text: string, facet: "cites" | "citedby") =>
      MenuItem.new({ text, action: () => one && around(one, facet), enabled: knows });
    const separator = () => PredefinedMenuItem.new({ item: "Separator" });
    const menu = await Menu.new({
      items: [
        await item("open"),
        await item("add"),
        await item("addBare"),
        await separator(),
        await item("star"),
        await separator(),
        await aroundItem("Cited By", "cites"),
        await aroundItem("References", "citedby"),
      ],
    });
    await menu.popup();
  }

  /** ⌘F, /: the search field. */
  export function focusSearch() {
    list.focusSearch();
  }

  /** A key of the window while the tab is in front (none while a sheet is open): Enter in the fields runs the
   *  search; the list's keys move and read (`navKey`); the keys of the actions act on the selection (s, a, o). */
  export function key(e: KeyboardEvent) {
    if (modalOpen()) return;
    const act = (action: () => unknown) => {
      e.preventDefault();
      action();
    };
    const field = e.target === list.input || e.target === authorsInput || e.target === yearInput;
    if (field && e.key === "Enter" && !e.isComposing) return act(run);
    if (list.navKey(e) || typing(e) || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
    const action = Object.values(actions).find((a) => a.key === e.key);
    if (action) act(() => action.run(list.targets()));
  }

  /** An example of the help, in the search field. */
  function example(text: string) {
    search.query = text;
    search.authors = "";
    search.year = "";
    list.input?.focus();
  }
</script>

<main class="tab" style:--tint="var(--search)">
  {#if tab.showSidebar}
    <aside class="sidebar" style:width="{tab.sidebar}px">
      <SearchSidebar
        {view}
        total={results?.total ?? null}
        starred={starredCount}
        recent={search.recent}
        onview={showList}
        onrecent={runAgain}
        onclear={() => (search.recent = [])}
      />
    </aside>
    <Splitter bind:width={tab.sidebar} side="left" min={160} max={420} />
  {/if}
  <div class="right">
    <header class="two" bind:this={header}>
      {@render tab.tabs()}
      <div class="search" bind:this={searchBox}>
        <input
          bind:this={list.input}
          bind:value={search.query}
          type="search"
          placeholder={'Search OpenAlex and your library: words, "phrases", -word, t:, abs:, j:, doi:'}
          spellcheck="false"
          autocomplete="off"
        />
        {#if search.query}
          <button class="clear" aria-label="Clear the search field" onclick={() => ((search.query = ""), list.input?.focus())}>×</button>
        {/if}
      </div>
      <button class="tool primary" disabled={searching} title="Search OpenAlex (Enter in the fields)" onclick={run}>
        {searching ? "Searching…" : "Search"}
      </button>
      <button class="tool" title="Search online" onclick={onlineMenu}>Online ▾</button>
      <span class="notice">{list.notice ?? ""}</span>
      <span class="spacer"></span>
      <button
        class="tool"
        disabled={!results || subscribing}
        title={results
          ? `Subscribe to “${results.query}” in the Feeds tab: its new works come with the feeds (a sweep of OpenAlex)`
          : "Search first, then subscribe to the search in the Feeds tab"}
        onclick={subscribe}
      >
        {subscribing ? "Subscribing…" : "Subscribe to this search"}
      </button>
      <!-- the names and years apart, the author's field under the search field, its label under the tabs: OpenAlex
           matches words in the texts only -->
      <div class="fields">
        <label class="field"
          ><span class="name" style:width="{Math.max(0, fieldsLeft - 6)}px">Author(s)</span><input
            bind:this={authorsInput}
            bind:value={search.authors}
            class="authors"
            placeholder="Smith, Fricker"
            title="The authors of the works, each required: separated by commas, & or and (a: in the query)"
            spellcheck="false"
            autocomplete="off"
          /></label
        >
        <label class="field"
          >Year(s)<input
            bind:this={yearInput}
            bind:value={search.year}
            class="year"
            placeholder="2020, 2000-2010"
            title="The years of the works: 2005, 2000-2010, 2000-, -2010, or several: 2015, 2020 (y: in the query)"
            spellcheck="false"
            autocomplete="off"
          /></label
        >
        <span class="explain"
          >OpenAlex looks for the words in titles, abstracts and full texts only: names and years go in their fields.
          Online ▾ sends all of it as one query.</span
        >
      </div>
    </header>
    <ItemPane
      {list}
      table={{
        columns: search.columns,
        specs: SEARCH_SPECS,
        menu: SEARCH_ORDER,
        date: "year",
        unread: false,
        star: SAVED_FOR_LATER.row,
        after: view === "results" && results?.next ? showMore : undefined,
        oncolumns: (shown) => (search.columns = shown),
        onstar: (id) => actions.star.run([id]),
        onmenu: rowMenu,
      }}
      tint="var(--search)"
      none="No work selected"
      bar={listBar}
      empty={blank ? noRows : undefined}
      actions={detailButtons}
    />
    <footer class="status">
      <span>{searching ? "Searching OpenAlex…" : "OpenAlex's works"}</span>
      <span class="spacer"></span>
      <span>Saved for later {starredCount}</span>
    </footer>
  </div>
</main>

<!-- above the list: the search shown as a chip, the count and the order, the list simple or a table; the selection
     bar -->
{#snippet listBar()}
  <div class="filters">
    {#if view === "starred"}
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
      <button class="sort" title="The order of the works found: the newest, the best score, the most cited first, or as OpenAlex ranks them" onclick={orderMenu}>
        {orderText} ▾
      </button>
    {/if}
    <LookSwitch bind:look={search.look} />
    {#if list.selection.selected.size > 1}
      {@const ids = list.chosen()}
      {@const items = list.itemsOf(ids)}
      <div class="selbar">
        <span>{list.selection.selected.size} selected</span>
        {#each BAR_BUTTONS as name (name)}
          {@const action = actions[name]}
          <button disabled={!action.enabled(items)} onclick={() => action.run(ids)}>{buttonText(name, action, items)}</button>
        {/each}
        <span class="spacer"></span>
        <button class="plain" onclick={() => list.selection.only(null)}>Clear selection</button>
      </div>
    {/if}
  </div>
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
      {view === "starred"
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

<!-- the buttons of the details: those of the table, on the selection when the work is in it; the works around it -->
{#snippet detailButtons(d: FeedDetail)}
  {@const ids = list.targetsOr(d.id)}
  <!-- the works around it are searched by its OpenAlex id, else its DOI -->
  {@const unknown = !d.openalex && !d.doi}
  <ActionButtons {actions} names={BUTTONS} menus={{ add: ["addBare"] }} {ids} items={list.itemsOf(ids)} owned={ownedOf(tab, d, ids)} />
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
      <code>y:</code> years, <code>doi:</code>. The Author(s) field adds <code>a:</code> (Smith, Fricker: both), the
      Year(s) field <code>y:</code> (2005, 2000-2010, 2000-, -2010, 2015, 2020). The papers of your library closest to
      the search come too (✓).
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
      <b>Subscribe to this search</b> brings its new works into the Feeds tab.
    </p>
  </div>
{/snippet}

<style>
  /* two lines: the tabs, the search field and its buttons; under them the names and years, the Author(s) field
     under the search field (`fieldsLeft`) */
  header.two {
    flex-wrap: wrap;
    row-gap: 6px;
    height: auto;
    padding-top: 8px;
    padding-bottom: 8px;
  }
  .fields {
    flex: 1 0 100%;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .field {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
    font-size: 12px;
  }
  .field .name {
    text-align: right;
  }
  .field input {
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--fg);
    font: inherit;
    font-size: 12px;
  }
  .field input:focus {
    border-color: var(--search);
    outline: none;
  }
  input.authors {
    width: 200px;
  }
  /* the years: room for 2000-2010, 2020 */
  input.year {
    width: 120px;
    font-variant-numeric: tabular-nums;
  }
  .explain {
    flex: 1;
    min-width: 0;
    color: var(--muted);
    font-size: 11px;
    line-height: 1.3;
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
