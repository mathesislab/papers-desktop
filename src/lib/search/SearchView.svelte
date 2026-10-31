<script lang="ts">
  // The Search tab of the main window: a search of OpenAlex's works, as a search engine of papers, to find papers
  // and add them to the library. The query is the app's (words, "phrases", -word, t:, abs:, a:, j:, y:, doi:, and
  // cites:, citedby: for the works around a work), the Year field added as y:; Enter runs it, a page of 25 works at
  // a time (*Show 25 more* at the end of the list). A new search takes the place of the works found before, but for
  // those saved for later (the star, S) or added. The sidebar: the works found (Results), those saved for later, the
  // recent searches. The list and the details are the Feeds tab's (FeedTable, FeedDetails, Reader), with the store
  // "search": the details beside the list or in place of it (the setting `feeds_beside`), as in the Feeds tab. The
  // window (src/routes/library/+page.svelte) keeps the layout (the fields, the recent searches, the search shown),
  // and passes on the commands of the menu and the keys while the tab is in front. The Add sheet is the library's.

  import { listen } from "@tauri-apps/api/event";
  import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount, type Snippet } from "svelte";
  import * as feedsApi from "$lib/feeds/api";
  import type { FeedDetail, FeedRow, PdfStatus, Triage } from "$lib/feeds/api";
  import FeedDetails from "$lib/feeds/FeedDetails.svelte";
  import FeedTable, { SEARCH_ORDER, type FeedColumn } from "$lib/feeds/FeedTable.svelte";
  import { interestColours } from "$lib/feeds/filters";
  import { addTarget, mergeRows, pageOf, rangeOf, toggled } from "$lib/feeds/items";
  import Reader from "$lib/feeds/Reader.svelte";
  import { mac, matches, type Shortcut } from "$lib/keys";
  import { modalOpen } from "$lib/library/modal.svelte";
  import { OPEN_ASK } from "$lib/library/selection";
  import Splitter from "$lib/library/Splitter.svelte";
  import * as api from "./api";
  import {
    ORDERS,
    aroundQuery,
    appended,
    composed,
    moreText,
    totalText,
    withRecent,
    yearError,
    type Asked,
    type SearchState,
  } from "./query";
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
    /** the widths of the list's columns: the window's layout keeps them */
    columns: Record<FeedColumn, number>;
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

  const ALL: Shortcut = { key: "a", mod: true };
  /** how long an item is shown before its abstract is looked up: j held down does not ask for each */
  const LOOKUP_DELAY = 300;

  /** the list shown: the works of the search, or those saved for later */
  let view = $state<"results" | "saved">("results");
  let rows = $state<FeedRow[]>([]);
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
  let selected = $state(new Set<string>());
  /** the item the details show, and that the keys move from */
  let focus = $state<string | null>(null);
  /** where shift extends the selection from */
  let anchor: string | null = null;
  let detail = $state<FeedDetail | null>(null);
  /** the details beside the list (the setting), else in place of it */
  let beside = $state(false);
  /** the item in focus read in place of the list */
  let reading = $state(false);
  /** the details are asked again: the works changed */
  let version = $state(0);
  /** the colours of the interests, as in the Feeds tab */
  let colours = $state(new Map<string, string>());
  let input = $state<HTMLInputElement>();
  let yearInput = $state<HTMLInputElement>();
  /** what was just done, said for a moment */
  let notice = $state<string | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout>;
  /** the works unsaved in *Saved for later*: kept in place until another list is shown (`mergeRows`) */
  const sticky = new Set<string>();
  /** the free PDFs asked for, by item */
  const pdfs = new Map<string, Promise<PdfStatus>>();
  /** the items whose abstract was looked up here, once each */
  const lookedUp = new Set<string>();
  /** the searches asked: an answer to one before the last is dropped */
  let asked = 0;
  /** bumped when the list is replaced (a search, a page, another list): a refresh asked before it is dropped */
  let generation = 0;

  let byId = $derived(new Map(rows.map((r) => [r.id, r])));
  /** the row of the item in focus, pinned over the item read */
  let focusRow = $derived(focus ? byId.get(focus) : undefined);
  let results = $derived(search.results);
  /** the query whose words the rows and the details mark */
  let marked = $derived(view === "results" ? (results?.query ?? "") : "");
  let yearWrong = $derived(yearError(search.year));
  let orderText = $derived(ORDERS.find((o) => o.order === search.order)?.text ?? "");
  let summary = $derived(
    (view === "saved"
      ? `${rows.length} saved`
      : results
        ? totalText(results.total) + (results.ids.length < results.total ? ` · ${results.ids.length} shown` : "")
        : "") + (selected.size > 1 ? ` · ${selected.size} selected` : ""),
  );

  onMount(() => {
    const unlisten = [
      // a triage, an abstract looked up, the library indexed again
      listen(api.EVENTS.changed, () => {
        version++;
        refresh();
      }),
      listen<boolean>(feedsApi.EVENTS.beside, (e) => (beside = e.payload)),
    ];
    feedsApi.besideShown().then((b) => (beside = b), (e) => say(String(e)));
    feedsApi.settings().then(
      (s) => (colours = interestColours(s.interests.map((i) => i.name))),
      () => {},
    );
    refresh();
    return () => unlisten.forEach((u) => u.then((f) => f()));
  });

  // the details of the item in focus, marked by the query; again when the works change
  $effect(() => {
    const id = focus;
    const query = marked;
    version;
    if (!id) {
      detail = null;
      return;
    }
    let live = true;
    feedsApi
      .detail(id, query, "search")
      .then((d) => live && (detail = d))
      .catch(() => live && (detail = null));
    return () => {
      live = false;
    };
  });

  // the abstract of the item shown, looked up if its lookups may still find it (the works then change)
  $effect(() => {
    const id = detail?.abstractPending ? detail.id : null;
    if (!id || lookedUp.has(id)) return;
    const timer = setTimeout(() => {
      lookedUp.add(id);
      feedsApi.lookUp(id, "search").catch((e) => say(String(e)));
    }, LOOKUP_DELAY);
    return () => clearTimeout(timer);
  });

  function say(text: string) {
    notice = text;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = null), 4000);
  }

  /** The list again as the works are now: those of the search shown, found again, or those saved for later (those
   *  unsaved here kept in place); and how many are saved. */
  async function refresh() {
    const g = generation;
    const shown = view;
    const r = results;
    try {
      const saved = await api.saved();
      const fresh = shown === "saved" ? saved : r ? await api.rowsOf(r.ids, r.query) : [];
      savedCount = saved.length;
      if (g !== generation) return;
      rows = shown === "saved" ? mergeRows(rows, fresh, sticky) : fresh;
    } catch (e) {
      say(String(e));
    }
    loaded = true;
    const ids = new Set(rows.map((r) => r.id));
    if ([...selected].some((id) => !ids.has(id))) selected = new Set([...selected].filter((id) => ids.has(id)));
  }

  /** Another list, from the sidebar: nothing selected, the list as it is now. */
  function showList(to: "results" | "saved") {
    generation++;
    view = to;
    sticky.clear();
    selected = new Set();
    anchor = focus = null;
    reading = false;
    refresh();
  }

  /** Enter, Search: the first page of the query of the fields, in place of the works found before (but for those
   *  saved for later or added); kept in the recent searches. A year that is not one, or a query OpenAlex cannot be
   *  asked, is said where the works would be. */
  async function run() {
    const query = composed(search.query, search.year);
    if (query === null) {
      view = "results";
      failure = yearWrong;
      return;
    }
    if (!query) return focusSearch();
    const typed: Asked = { query: search.query, year: search.year, order: search.order };
    const n = ++asked;
    generation++;
    view = "results";
    sticky.clear();
    reading = false;
    searching = true;
    failure = null;
    try {
      const page = await api.run(query, typed.order, null);
      if (n !== asked) return;
      search.results = { query, order: typed.order, ids: page.rows.map((r) => r.id), total: page.total, next: page.next };
      search.recent = withRecent(search.recent, typed);
      rows = page.rows;
      selected = new Set();
      anchor = focus = null;
    } catch (e) {
      if (n === asked) failure = String(e);
    } finally {
      if (n === asked) {
        searching = false;
        // a refresh asked while the works were replaced would bring the old ones back
        generation++;
      }
    }
    refresh();
  }

  /** Show 25 more: the next page of the search shown, after the works shown. */
  async function loadMore() {
    const r = results;
    if (!r?.next || more || searching) return;
    more = true;
    try {
      const page = await api.run(r.query, r.order, r.next);
      if (results !== r) return;
      r.ids = [...new Set([...r.ids, ...page.rows.map((w) => w.id)])];
      r.total = page.total;
      r.next = page.next;
      if (view === "results") rows = appended(rows, page.rows);
    } catch (e) {
      say(String(e));
    } finally {
      more = false;
      generation++;
    }
  }

  /** A recent search, or the works around a work, in the fields, run. */
  function runAgain(again: Asked) {
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
    const choose = (order: Asked["order"]) => {
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
    generation++;
    search.query = "";
    search.year = "";
    search.results = null;
    rows = [];
    failure = null;
    selected = new Set();
    anchor = focus = null;
    reading = false;
    focusSearch();
  }

  /** Follow this search: a sweep of OpenAlex in the Feeds tab, fetched at once. */
  async function follow() {
    const query = results?.query;
    if (!query || following) return;
    following = true;
    try {
      const info = await api.follow(query);
      say(`Following "${info.title}" in Feeds`);
      feedsApi.fetchFeeds(info.id).catch((e) => say(String(e)));
    } catch (e) {
      say(String(e));
    } finally {
      following = false;
    }
  }

  /** The details of an item: those shown, else asked. */
  async function detailOf(id: string): Promise<FeedDetail | null> {
    if (detail?.id === id) return detail;
    try {
      return await feedsApi.detail(id, "", "search");
    } catch {
      return null;
    }
  }

  /** The free PDF of an item, asked once. */
  function pdfOf(id: string): Promise<PdfStatus> {
    let found = pdfs.get(id);
    if (!found) {
      found = feedsApi.pdfStatus(id, "search");
      found.catch(() => pdfs.delete(id));
      pdfs.set(id, found);
    }
    return found;
  }

  /** The items the keys and the menu act on: the selection when the focus is in it, else the item in focus. */
  function targets(): string[] {
    if (focus && selected.has(focus)) return rows.filter((r) => selected.has(r.id)).map((r) => r.id);
    return focus ? [focus] : [];
  }

  /** The items the buttons of the details act on: the keys', else the item shown. */
  const targetsOr = (id: string) => (targets().length ? targets() : [id]);

  /** A triage given to works, shown at once (saved for later: shortlisted; added); then the list as it is. */
  async function setTo(ids: string[], triage: Triage) {
    if (!ids.length) return;
    const set = new Set(ids);
    if (view === "saved") for (const id of ids) sticky.add(id);
    rows = rows.map((r) => (set.has(r.id) ? { ...r, triage } : r));
    if (detail && set.has(detail.id)) detail = { ...detail, triage };
    try {
      await feedsApi.setTriage(ids, triage, "search");
    } catch (e) {
      say(String(e));
    }
    await refresh();
  }

  /** s, the star: the works saved for later, or no longer when all are (`toggled`); those added keep being added. */
  async function save(ids = targets()) {
    const found = await Promise.all(ids.map((id) => byId.get(id) ?? detailOf(id)));
    const items = found.filter((r): r is FeedRow | FeedDetail => !!r && r.triage !== "added");
    const change = toggled(items, "shortlist");
    if (!change) return;
    const n = change.ids.length === 1 ? "" : ` ${change.ids.length}`;
    say(change.triage === "shortlisted" ? `Saved${n} for later` : `No longer saved${n}`);
    await setTo(change.ids, change.triage);
  }

  /** o, a double-click: the page of each work in the browser (above 5, after asking). */
  async function openItems(ids: string[]) {
    if (!ids.length) return;
    const question = `Open ${ids.length} pages in the browser?`;
    if (ids.length > OPEN_ASK && !(await ask(question, { title: "Open", okLabel: "Open", cancelLabel: "Cancel" }))) return;
    let none = 0;
    for (const id of ids) {
      const d = await detailOf(id);
      const url = d && pageOf(d);
      if (url) await openUrl(url).catch((e) => say(String(e)));
      else none++;
    }
    if (none) say(`${none} without a link`);
  }

  /** a, Add: each work added in turn, by the Add sheet over the tab, and then *added*; a sheet closed stops the
   *  rest. */
  async function addItems(ids: string[], pdf: boolean) {
    for (const id of ids) {
      const d = await detailOf(id);
      const target = d && addTarget(d);
      if (!target) {
        say("No DOI or arXiv id to add it by");
        if (ids.length === 1) return;
        continue;
      }
      const said = await onadd({ id: target.id, pdf });
      if (said === null) return;
      say(said);
      if (target.item) await setTo([target.item], "added");
    }
  }

  /** An item to show, from the details (another version): selected, in the list or not. */
  function showItem(id: string) {
    selected = new Set([id]);
    anchor = focus = id;
  }

  function select(id: string, e: MouseEvent) {
    if (e.button !== 0) return;
    if (e.shiftKey && anchor) {
      selected = new Set(rangeOf(rows.map((r) => r.id), anchor, id));
    } else if (mac ? e.metaKey : e.ctrlKey) {
      const s = new Set(selected);
      if (s.has(id)) s.delete(id);
      else s.add(id);
      selected = s;
      anchor = id;
    } else {
      selected = new Set([id]);
      anchor = id;
      if (!beside) reading = true;
    }
    focus = id;
  }

  /** Move the focus by some rows, or to the first or last; extending the selection, or not. */
  function move(to: number | "first" | "last", extend: boolean) {
    const ids = rows.map((r) => r.id);
    if (!ids.length) return;
    const at = focus ? ids.indexOf(focus) : -1;
    const index =
      to === "first" ? 0 : to === "last" ? ids.length - 1 : at < 0 ? 0 : Math.max(0, Math.min(ids.length - 1, at + to));
    const next = ids[index];
    if (extend && anchor) {
      selected = new Set(rangeOf(ids, anchor, next));
    } else {
      selected = new Set([next]);
      anchor = next;
    }
    focus = next;
  }

  /** Right-click on a row: the works selected when it is one of them, else that work alone. */
  async function rowMenu(id: string) {
    if (!selected.has(id)) showItem(id);
    const ids = targets();
    const items = ids.map((i) => byId.get(i)).filter((r): r is FeedRow => r !== undefined);
    if (!items.length) return;
    const savable = items.filter((r) => r.triage !== "added");
    const one = ids.length === 1 ? await detailOf(id) : null;
    const knows = !!one && !!(one.openalex ?? one.doi);
    const item = (text: string, action: () => void, enabled = true) => MenuItem.new({ text, action, enabled });
    const separator = () => PredefinedMenuItem.new({ item: "Separator" });
    const menu = await Menu.new({
      items: [
        await item("Open", () => openItems(ids)),
        await item("Add…", () => addItems(ids, true)),
        await item("Add without PDF…", () => addItems(ids, false)),
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
    input?.select();
  }

  /** the rows a page key moves: about those in view */
  const pageRows = () => Math.max(1, Math.floor((window.innerHeight - 160) / 26));

  /** A key of the window while the tab is in front (none while a sheet is open): the arrows, j and k, the page keys,
   *  Home and End move (⇧ extends the selection with the arrows); s, a, o act on the selection; Enter reads the work
   *  in place of the list, Esc goes back; / goes to the search field, where Enter runs the search and Esc leaves. */
  export function key(e: KeyboardEvent) {
    if (modalOpen()) return;
    const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement;
    const mod = mac ? e.metaKey : e.ctrlKey;
    const go = (to: number | "first" | "last", extend = e.shiftKey) => {
      e.preventDefault();
      move(to, extend);
    };
    const act = (action: () => unknown) => {
      e.preventDefault();
      action();
    };
    if (e.key === "ArrowDown") return go(mod ? "last" : 1);
    if (e.key === "ArrowUp") return go(mod ? "first" : -1);
    if (e.key === "PageDown") return go(pageRows());
    if (e.key === "PageUp") return go(-pageRows());
    if (typing) {
      const field = e.target === input || e.target === yearInput;
      if (field && e.key === "Enter" && !e.isComposing) act(run);
      else if (e.key === "Escape") act(() => (e.target as HTMLElement).blur());
      return;
    }
    if (matches(e, ALL)) return act(() => (selected = new Set(rows.map((r) => r.id))));
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    // a / typed with ⇧ on some keyboards
    if (e.key === "/") return act(focusSearch);
    if (e.shiftKey) return;
    switch (e.key) {
      case "Enter":
        return beside || !focus ? undefined : act(() => (reading = !reading));
      case "Escape":
        return reading ? act(() => (reading = false)) : undefined;
      case "Home":
        return go("first");
      case "End":
        return go("last");
      case "j":
        return go(1, false);
      case "k":
        return go(-1, false);
      case "s":
        return act(() => save());
      case "a":
        return act(() => addItems(targets(), true));
      case "o":
        return act(() => openItems(targets()));
    }
  }

  /** The selection bar: the rows selected, in the order of the list. */
  const chosen = () => rows.filter((r) => selected.has(r.id)).map((r) => r.id);

  /** An example of the help, in the search field. */
  function example(text: string) {
    search.query = text;
    search.year = "";
    input?.focus();
  }
</script>

<main>
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
          bind:this={input}
          bind:value={search.query}
          type="search"
          placeholder={'Search OpenAlex: words, "phrases", -word, t:, abs:, a:, j:, doi:'}
          spellcheck="false"
          autocomplete="off"
        />
        {#if search.query}
          <button class="clear" aria-label="Clear the search field" onclick={() => ((search.query = ""), input?.focus())}>×</button>
        {/if}
      </div>
      <input
        bind:this={yearInput}
        bind:value={search.year}
        class="year"
        class:wrong={yearWrong !== null}
        placeholder="Year"
        title={yearWrong ?? "The years of the works: 2005, 2000-2010, 2000- or -2010 (y: in the query)"}
        spellcheck="false"
        autocomplete="off"
      />
      <button class="tool primary" disabled={searching} title="Search OpenAlex (Enter in the fields)" onclick={run}>
        {searching ? "Searching…" : "Search"}
      </button>
      <span class="notice">{notice ?? ""}</span>
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
    <div class="content">
      <section class="list">
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
        {#if selected.size > 1}
          <div class="selbar">
            <span>{selected.size} selected</span>
            <button onclick={() => addItems(chosen(), true)}>Add…</button>
            <button onclick={() => save(chosen())}>☆ Save for later</button>
            <button onclick={() => openItems(chosen())}>Open ↗</button>
            <span class="spacer"></span>
            <button class="plain" onclick={() => (selected = new Set(focus ? [focus] : []))}>Clear selection</button>
          </div>
        {/if}
        {#if view === "results" && searching}
          <p class="empty"><span class="spin">↻</span> Searching OpenAlex…</p>
        {:else if view === "results" && failure}
          <p class="empty error">{failure}</p>
        {:else if !loaded}
          <p class="empty"></p>
        {:else if view === "results" && !results}
          {@render help()}
        {:else if !rows.length}
          <p class="empty">
            {view === "saved"
              ? "Nothing saved for later. ☆ or S keeps a work found, to come back to it without searching again."
              : "No works found."}
          </p>
        {:else}
          <div class="rows">
            <FeedTable
              {rows}
              {selected}
              {focus}
              columns={SEARCH_ORDER}
              names={{ journal: "Journal" }}
              widths={columns}
              {colours}
              unread={false}
              star={{ on: "Saved for later: unsave", off: "Save for later" }}
              after={view === "results" && results?.next ? showMore : undefined}
              onwidths={(widths) => (columns = widths)}
              onstar={(id) => save([id])}
              onselect={select}
              onopen={(id) => openItems(selected.has(id) ? targets() : [id])}
              onmenu={rowMenu}
            />
            {#if reading && !beside && focusRow}
              <Reader row={focusRow} tint="var(--search)" onback={() => (reading = false)}>
                {#if detail}{@render itemDetails(detail, true)}{/if}
              </Reader>
            {/if}
          </div>
        {/if}
      </section>
      {#if beside && showDetails}
        <Splitter bind:width={details} side="right" min={260} max={600} />
        <aside class="details" style:width="{details}px">
          {#if detail}
            {@render itemDetails(detail, false)}
          {:else}
            <p class="empty">No work selected</p>
          {/if}
        </aside>
      {/if}
    </div>
    <footer class="status">
      <span>{searching ? "Searching OpenAlex…" : "OpenAlex's works"}</span>
      <span class="spacer"></span>
      <span>Saved for later {savedCount}</span>
    </footer>
  </div>
</main>

{#snippet showMore()}
  {#if results}
    <div class="more">
      <button disabled={more} onclick={loadMore}>{more ? "Loading…" : moreText(results.ids.length, results.total)}</button>
      <span>{results.ids.length.toLocaleString("en-US")} of {totalText(results.total)}</span>
    </div>
  {/if}
{/snippet}

{#snippet itemDetails(d: FeedDetail, wide: boolean)}
  <FeedDetails
    detail={d}
    {wide}
    count={focus && selected.has(focus) ? selected.size : 1}
    {byId}
    {colours}
    {pdfOf}
    actions={itemActions}
    onitem={showItem}
    {onentry}
    onback={wide ? () => (reading = false) : undefined}
  />
{/snippet}

<!-- the buttons of the details: add, open, save for later, the works around it -->
{#snippet itemActions(d: FeedDetail)}
  <!-- the works around it are searched by its OpenAlex id, else its DOI -->
  {@const unknown = !d.openalex && !d.doi}
  <button class="primary" onclick={() => addItems(targetsOr(d.id), true)} title="Add to the library, with the free PDF if there is one">Add <kbd>A</kbd></button>
  <button class="open" onclick={() => openItems(targetsOr(d.id))} disabled={!d.url && !d.doi} title="Open its page">Open ↗ <kbd>O</kbd></button>
  <button onclick={() => addItems(targetsOr(d.id), false)} title="Add to the library without looking for its PDF">Add without PDF</button>
  {#if d.triage !== "added"}
    <button onclick={() => save(targetsOr(d.id))} title="Keep it, to find it again without searching: a new search does not drop it">
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
  main {
    display: flex;
    height: 100vh;
  }

  .sidebar {
    flex: none;
    overflow-y: auto;
    background: var(--sidebar);
    border-right: 1px solid var(--line);
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
  /* the table, and the work read over it: the table stays as it was, scrolled where it was */
  .rows {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  /* the search keeps its width: what is next to it stays in place */
  .search {
    position: relative;
    flex: 0 1 50%;
    min-width: 200px;
    max-width: 560px;
    display: flex;
  }
  input {
    flex: 1;
    min-width: 0;
    height: 28px;
    padding: 0 24px 0 10px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: transparent;
    color: var(--fg);
    font: inherit;
    outline: none;
  }
  input:focus {
    border-color: var(--accent);
  }
  input::-webkit-search-cancel-button {
    display: none;
  }
  /* the years: room for 2000-2010 */
  input.year {
    flex: none;
    width: 92px;
    padding: 0 8px;
    font-variant-numeric: tabular-nums;
  }
  input.year.wrong {
    border-color: var(--error);
  }
  .clear {
    position: absolute;
    top: 4px;
    right: 4px;
    width: 20px;
    height: 20px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: none;
    color: var(--muted);
    font-size: 15px;
    line-height: 20px;
  }
  .clear:hover {
    background: var(--line);
    color: var(--fg);
  }
  .notice {
    min-width: 0;
    overflow: hidden;
    color: var(--muted);
    font-size: 12px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .spacer {
    flex: 1;
  }
  .tool {
    flex: none;
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
    white-space: nowrap;
  }
  .tool:disabled {
    opacity: 0.6;
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
    flex: none;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 6px;
    min-height: 20px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--line);
    font-size: 12px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    max-width: min(480px, 60%);
    padding: 1px 8px;
    border: 1px solid color-mix(in srgb, var(--search) 40%, transparent);
    border-radius: 10px;
    background: color-mix(in srgb, var(--search) 14%, transparent);
    color: var(--fg);
  }
  .chip:has(button) {
    padding-right: 2px;
  }
  .chip .text {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .chip button {
    flex: none;
    width: 16px;
    height: 16px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: none;
    color: var(--muted);
    font-size: 13px;
    line-height: 16px;
  }
  .chip button:hover {
    background: var(--line);
    color: var(--fg);
  }
  .count {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .sort {
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
    font: inherit;
  }
  .sort:hover {
    color: var(--fg);
  }
  /* two or more rows selected: what acts on them all */
  .selbar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 12px;
    border-bottom: 1px solid var(--line);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    font-size: 12px;
  }
  .selbar button {
    padding: 2px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--fg);
    font: inherit;
  }
  .selbar button.plain {
    border: none;
    background: none;
    color: var(--muted);
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

  /* where the works come from, and what is saved */
  .status {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    height: 26px;
    padding: 0 12px;
    border-top: 1px solid var(--line);
    background: var(--sidebar);
    color: var(--muted);
    font-size: 12px;
    white-space: nowrap;
  }

  .empty {
    margin: auto;
    padding: 24px;
    color: var(--muted);
    text-align: center;
  }
  .empty.error {
    max-width: 560px;
    color: var(--error);
    -webkit-user-select: text;
    user-select: text;
  }
</style>
