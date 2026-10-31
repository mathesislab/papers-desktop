<script lang="ts">
  // The Feeds tab of the main window (docs/design.md, Feeds; the mockup docs/mockups/feeds.html): the sidebar (the
  // views, kinds, interests and sources), the list of the items its choices, the segment and the search select, the
  // details of the one selected, beside the list or in place of it (the setting `feeds_beside`: a click or Enter opens
  // the item over the list, its row pinned at the top; Esc or the row goes back to the list, where it was); the keys
  // of the triage; the Review of the starred items, over the list and the details; the Subscribe sheet.
  // The window (src/routes/library/+page.svelte) keeps the
  // layout, and passes on the commands of the menu and the keys while the tab is in front. The Add sheet is the
  // library's: it opens over this tab, which stays in front after the paper is added.

  import { listen } from "@tauri-apps/api/event";
  import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu } from "@tauri-apps/api/menu";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onDestroy, onMount, untrack, type Snippet } from "svelte";
  import { label, mac, matches, type Shortcut } from "$lib/keys";
  import Marked from "$lib/Marked.svelte";
  import { modalOpen } from "$lib/library/modal.svelte";
  import { OPEN_ASK } from "$lib/library/selection";
  import Splitter from "$lib/library/Splitter.svelte";
  import { added, changed, refusal, removed, renamed, sameName } from "$lib/saved";
  import SavedSearches from "$lib/SavedSearches.svelte";
  import SaveSearch from "$lib/SaveSearch.svelte";
  import * as api from "./api";
  import type { FeedCounts, FeedDetail, FeedRow, FetchSummary, Linked, SubscriptionInfo, Triage } from "./api";
  import FeedDetails from "./FeedDetails.svelte";
  import FeedsSidebar from "./FeedsSidebar.svelte";
  import FeedTable, { type FeedColumn } from "./FeedTable.svelte";
  import Review from "./Review.svelte";
  import SubscribeSheet from "./SubscribeSheet.svelte";
  import {
    SEGMENTS,
    SOURCE_KINDS,
    SORTS,
    applied,
    chooseInterest,
    chooseKind,
    chooseSegment,
    chooseSource,
    feedChips,
    interestColours,
    isPost,
    nextSort,
    isShown,
    queryOf,
    requestOf,
    searchOf,
    sourceGroups,
    withoutChip,
    type FeedChip,
    type FeedFilters,
  } from "./filters";
  import {
    addTarget,
    fetchLine,
    mergeRows,
    newAbove,
    newOlderThan,
    nextAfter,
    pageOf,
    rangeOf,
    stale,
    toggled,
    type TriageAction,
  } from "./items";
  import { Dwell } from "./review";

  let {
    tabs,
    filters = $bindable(),
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
    /** the choices of the tab, its search and sort: the window's layout keeps them */
    filters: FeedFilters;
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

  const FETCH: Shortcut = { key: "r", mod: true };
  const ALL: Shortcut = { key: "a", mod: true };
  const REVIEW: Shortcut = { key: "Enter", mod: true };

  let rows = $state<FeedRow[]>([]);
  let counts = $state<FeedCounts | null>(null);
  let subscriptions = $state<SubscriptionInfo[]>([]);
  /** the first answer came */
  let loaded = $state(false);
  let failure = $state<string | null>(null);
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
  /** the details are asked again: papers found for a post, items changed */
  let version = $state(0);
  /** the post whose papers are being looked for */
  let finding = $state<string | null>(null);
  let last = $state<FetchSummary | null>(null);
  let fetching = $state(false);
  /** the failures of the last fetch, listed */
  let failures = $state(false);
  let input = $state<HTMLInputElement>();
  /** the Review of the shortlist, over the list and the details */
  let reviewing = $state(false);
  let reviewer = $state<ReturnType<typeof Review>>();
  /** the Subscribe sheet, open; whether a subscription was made in it (fetched when it closes) */
  let subscribing = $state(false);
  let subscribed = false;
  /** what was just done, said for a moment */
  let notice = $state<string | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout>;
  let now = $state(Date.now());
  /** the items triaged here since the filters last changed: the list keeps them in place (`mergeRows`) */
  const sticky = new Set<string>();
  /** the free PDFs asked for, by item */
  const pdfs = new Map<string, Promise<api.PdfStatus>>();
  /** above this many, Mark read asks first */
  const MARK_ASK = 50;
  /** the items whose abstract was looked up here, once each */
  const lookedUp = new Set<string>();
  /** how long an item is shown before its abstract is looked up: j held down does not ask for each */
  const LOOKUP_DELAY = 300;

  let byId = $derived(new Map(rows.map((r) => [r.id, r])));
  /** the row of the item in focus, pinned over the item read */
  let focusRow = $derived(focus ? byId.get(focus) : undefined);
  let request = $derived(requestOf(filters));
  let chips = $derived(feedChips(filters));
  let groups = $derived(sourceGroups(subscriptions, counts?.sources ?? {}));
  let colours = $derived(interestColours(Object.keys(counts?.interests ?? {})));
  let shortlisted = $derived(counts?.is.shortlisted ?? 0);
  /** the saved searches of the sidebar; the one the tab shows */
  let searches = $state<api.SavedSearch[]>([]);
  let shownSearch = $derived(searches.find((s) => isShown(filters, s))?.name ?? null);

  /** An item becomes seen once it has stayed selected a moment (`Dwell`); one passed over stays new. */
  const dwell = new Dwell((id) => {
    if (byId.get(id)?.triage === "new") setTo([id], "seen");
  });

  onMount(() => {
    const unlisten = [
      // a fetch in the background (polling) or from here
      listen<boolean>(api.EVENTS.fetching, (e) => {
        fetching = e.payload;
        if (!e.payload) refresh();
      }),
      // a fetch, a triage from elsewhere, papers found
      listen(api.EVENTS.changed, () => {
        version++;
        load(true);
      }),
      listen<boolean>(api.EVENTS.beside, (e) => (beside = e.payload)),
    ];
    api.besideShown().then((b) => (beside = b), (e) => say(String(e)));
    const clock = setInterval(() => (now = Date.now()), 30000);
    start();
    return () => {
      clearInterval(clock);
      unlisten.forEach((u) => u.then((f) => f()));
    };
  });

  onDestroy(() => dwell.stop());

  /** The tab opened: the subscriptions, the saved searches, and a fetch when the last is older than 15 minutes. */
  async function start() {
    try {
      [last, subscriptions, searches] = await Promise.all([api.lastFetch(), api.subscriptions(), api.savedSearches()]);
    } catch (e) {
      failure = String(e);
    }
    if (stale(last) && subscriptions.length) fetchNow();
  }

  /** ⌘R, ↻: every subscription fetched, then the list again. */
  async function fetchNow() {
    if (fetching) return;
    fetching = true;
    failures = false;
    try {
      last = await api.fetchFeeds();
      const failed = last.failed.length;
      if (failed) say(`${failed} ${failed === 1 ? "subscription" : "subscriptions"} failed`);
    } catch (e) {
      say(String(e));
    } finally {
      fetching = false;
    }
    await refresh();
  }

  /** After a fetch: the subscriptions, the summary of the fetch, the list. */
  async function refresh() {
    try {
      [last, subscriptions] = await Promise.all([api.lastFetch(), api.subscriptions()]);
    } catch {
      // the list says what is wrong
    }
    await load(true);
  }

  let asked = 0;

  /** The items the filters select, and the counts; with `keep`, the items triaged here stay in their place. */
  async function load(keep: boolean) {
    const n = ++asked;
    try {
      const answer = await api.view(request);
      if (n !== asked) return;
      rows = keep ? mergeRows(rows, answer.rows, sticky) : answer.rows;
      counts = answer.counts;
      failure = null;
    } catch (e) {
      if (n !== asked) return;
      failure = String(e);
    }
    loaded = true;
    const ids = new Set(rows.map((r) => r.id));
    if ([...selected].some((id) => !ids.has(id))) selected = new Set([...selected].filter((id) => ids.has(id)));
    if (!keep && focus && !ids.has(focus)) focus = null;
  }

  // the filters changed: the list again, the items triaged before let go
  $effect(() => {
    request;
    untrack(() => {
      sticky.clear();
      load(false);
    });
  });

  // the details of the item in focus, marked by the query; again when items change
  $effect(() => {
    const id = focus;
    const query = request.query;
    version;
    if (!id) {
      detail = null;
      return;
    }
    let live = true;
    api
      .detail(id, query)
      .then((d) => live && (detail = d))
      .catch(() => live && (detail = null));
    return () => {
      live = false;
    };
  });

  // the abstract of the item shown, looked up if its lookups may still find it (the feeds then change)
  $effect(() => {
    const id = detail?.abstractPending ? detail.id : null;
    if (!id || lookedUp.has(id)) return;
    const timer = setTimeout(() => {
      lookedUp.add(id);
      api.lookUp(id).catch((e) => say(String(e)));
    }, LOOKUP_DELAY);
    return () => clearTimeout(timer);
  });

  // the item in focus, if new, is seen once it stays
  $effect(() => {
    const id = focus;
    untrack(() => dwell.show(id, id !== null && byId.get(id)?.triage === "new"));
  });

  /** The details of an item: those shown, else asked. */
  async function detailOf(id: string): Promise<FeedDetail | null> {
    if (detail?.id === id) return detail;
    try {
      return await api.detail(id, "");
    } catch {
      return null;
    }
  }

  /** The free PDF of an item, asked once. */
  function pdfOf(id: string): Promise<api.PdfStatus> {
    let found = pdfs.get(id);
    if (!found) {
      found = api.pdfStatus(id);
      found.catch(() => pdfs.delete(id));
      pdfs.set(id, found);
    }
    return found;
  }

  function say(text: string) {
    notice = text;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = null), 4000);
  }

  /** The items the keys and the menu act on: the selection when the focus is in it, else the item in focus. */
  function targets(): string[] {
    if (focus && selected.has(focus)) return rows.filter((r) => selected.has(r.id)).map((r) => r.id);
    return focus ? [focus] : [];
  }

  /** A triage given to items. Shown at once; the list keeps them in place until the filters change, except
   *  dismissed items, which go, the next selected. Then the counts, and the list, again. */
  async function setTo(ids: string[], triage: Triage) {
    if (!ids.length) return;
    const set = new Set(ids);
    if (triage === "dismissed") {
      const next = focus && set.has(focus) ? nextAfter(rows.map((r) => r.id), set) : focus;
      for (const id of ids) sticky.delete(id);
      rows = rows.filter((r) => !set.has(r.id));
      if (next !== focus) {
        selected = new Set(next ? [next] : []);
        anchor = focus = next;
      } else {
        selected = new Set([...selected].filter((id) => !set.has(id)));
      }
    } else {
      for (const id of ids) sticky.add(id);
      rows = rows.map((r) => (set.has(r.id) ? { ...r, triage } : r));
    }
    if (detail && set.has(detail.id)) detail = { ...detail, triage };
    try {
      await api.setTriage(ids, triage);
    } catch (e) {
      say(String(e));
    }
    await load(true);
  }

  /** s, x, m: the triage of the items selected toggled (`toggled`); an item not in the list (a linked paper) is
   *  asked for. */
  async function triage(action: TriageAction, ids = targets()) {
    const found = await Promise.all(ids.map((id) => byId.get(id) ?? detailOf(id)));
    const items = found.filter((r): r is FeedRow | FeedDetail => !!r);
    const change = toggled(items, action);
    if (change) setTo(change.ids, change.triage);
  }

  /** ⇧Space, the menu: the new items above the one in focus (the menu's row) marked seen. */
  function markAbove(from = focus) {
    const ids = newAbove(rows, from);
    if (!ids.length) return;
    setTo(ids, "seen");
    say(`Marked ${ids.length} as seen`);
  }

  /** Mark read ▾: the new items of the list, those older than some days, those above the selection, marked seen
   *  (above 50, after asking). */
  async function markMenu() {
    const item = (text: string, ids: string[]) =>
      MenuItem.new({ text: `${text} (${ids.length})`, enabled: ids.length > 0, action: () => markSeen(ids) });
    const all = rows.filter((r) => r.triage === "new").map((r) => r.id);
    const older = (days: number) => newOlderThan(rows, days, Date.now());
    const menu = await Menu.new({
      items: [
        await item("All in This List", all),
        await PredefinedMenuItem.new({ item: "Separator" }),
        await item("Older than 1 Day", older(1)),
        await item("Older than 3 Days", older(3)),
        await item("Older than 1 Week", older(7)),
        await PredefinedMenuItem.new({ item: "Separator" }),
        await item("All Above the Selection", newAbove(rows, focus)),
      ],
    });
    await menu.popup();
  }

  async function markSeen(ids: string[]) {
    const question = `Mark ${ids.length} items as read?`;
    if (ids.length > MARK_ASK && !(await ask(question, { title: "Mark read", okLabel: "Mark read", cancelLabel: "Cancel" }))) return;
    setTo(ids, "seen");
    say(`Marked ${ids.length} as read`);
  }

  /** The selection bar: the rows selected, in the order of the list. */
  const chosen = () => rows.filter((r) => selected.has(r.id));

  /** o, a double-click: the page of each item in the browser (above 5, after asking); a new one is seen. */
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
    const fresh = ids.filter((id) => byId.get(id)?.triage === "new");
    if (fresh.length) setTo(fresh, "seen");
    if (none) say(`${none} without a link`);
  }

  /** a, the menu: each item added in turn, by the Add sheet over the tab; a sheet closed stops the rest. */
  async function addItems(ids: string[], pdf: boolean) {
    for (const id of ids) {
      const d = await detailOf(id);
      const target = d && addTarget(d);
      if (!target) {
        say(d && isPost(d.kind) ? "No paper linked to the post yet: F looks for it" : "No DOI or arXiv id to add it by");
        if (ids.length === 1) return;
        continue;
      }
      if (!(await add(target, pdf))) return;
    }
  }

  /** A paper added by its DOI (or arXiv id), the tab staying in front; its item, if it is one, becomes added.
   *  Whether it was added. */
  async function add(target: { id: string; item: string | null }, pdf: boolean): Promise<boolean> {
    const said = await onadd({ id: target.id, pdf });
    if (said === null) return false;
    say(said);
    if (target.item) await setTo([target.item], "added");
    return true;
  }

  /** f: the papers behind a post looked for again (its page, then Crossref by its title). */
  async function find(id = focus) {
    if (!id || finding) return;
    const d = await detailOf(id);
    if (!d || !isPost(d.kind)) return say("F finds the paper behind a post");
    finding = id;
    try {
      const found = await api.findPapers(id);
      say(found.length ? `Found ${found.length} ${found.length === 1 ? "paper" : "papers"}` : "No paper found");
      version++;
      await load(true);
    } catch (e) {
      say(String(e));
    } finally {
      finding = null;
    }
  }

  /** An item to show, from the details (a post that covers it, a linked paper): selected, in the list or not. */
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

  /** Right-click on a row: the items selected when it is one of them, else that item alone. */
  async function rowMenu(id: string) {
    if (!selected.has(id)) showItem(id);
    const ids = targets();
    const items = ids.map((i) => byId.get(i)).filter((r): r is FeedRow => r !== undefined);
    if (!items.length) return;
    const every = (t: Triage) => items.every((r) => r.triage === t);
    const item = (text: string, action: () => void, enabled = true) => MenuItem.new({ text, action, enabled });
    const separator = () => PredefinedMenuItem.new({ item: "Separator" });
    const posts = items.length === 1 && isPost(items[0].kind);
    // one item: the kind of each subscription that lists it, to set right
    const sources = items.length === 1 ? ((await detailOf(id))?.sources ?? []) : [];
    const kindOf = async (s: api.FeedDetail["sources"][number]) =>
      Submenu.new({
        text: sources.length === 1 ? "Source Is" : `${s.title} Is`,
        items: await Promise.all(
          SOURCE_KINDS.map((k) =>
            CheckMenuItem.new({ text: k.name, checked: s.kind === k.kind, action: () => setKind(s.id, s.title, k.kind) }),
          ),
        ),
      });
    const menu = await Menu.new({
      items: [
        await item(posts ? "Open Post" : "Open", () => openItems(ids)),
        await item("Add…", () => addItems(ids, true)),
        await item("Add without PDF…", () => addItems(ids, false)),
        ...(posts ? [await item("Find Linked Paper", () => find(id))] : []),
        await separator(),
        await item(every("shortlisted") ? "Unstar" : "Star", () => triage("shortlist", ids)),
        await item(items.some((r) => r.triage === "new") ? "Mark as Seen" : "Mark as New", () => triage("read", ids)),
        await item(every("dismissed") ? "Restore" : "Dismiss", () => triage("dismiss", ids)),
        await separator(),
        await item("Mark All Above as Seen", () => markAbove(id), newAbove(rows, id).length > 0),
        ...(sources.length ? [await separator(), ...(await Promise.all(sources.map(kindOf)))] : []),
      ],
    });
    await menu.popup();
  }

  /** A subscription set to another kind, from the row menu: its items move at once. */
  async function setKind(id: string, title: string, kind: api.SourceKind) {
    try {
      await api.updateSubscription(id, { kind });
      say(`${title}: ${SOURCE_KINDS.find((k) => k.kind === kind)?.name}`);
      version++;
      await load(true);
    } catch (e) {
      say(String(e));
    }
  }

  /** The Subscribe sheet closed: what was subscribed to is fetched. */
  function closeSubscribe() {
    subscribing = false;
    if (subscribed) fetchNow();
    subscribed = false;
  }

  /** ⌘↩: the Review of the shortlist; again, or Esc in it, the list. */
  function toggleReview() {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    reviewing = !reviewing;
    if (!reviewing) load(true);
  }

  /** ⌘F, /: the search field. */
  export function focusSearch() {
    input?.select();
  }

  /** the rows a page key moves: about those in view */
  const pageRows = () => Math.max(1, Math.floor((window.innerHeight - 160) / 26));

  /** A key of the window while the tab is in front (none while a sheet is open): ⌘R fetches, ⌘↩ starts the Review
   *  (which then has the keys); the arrows, j and k, the page keys, Home and End move (⇧ extends the selection with
   *  the arrows); s, x, m, o, a, f act on the selection; ⇧Space marks all above as seen; / goes to the search, Esc
   *  leaves it. */
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
    if (matches(e, FETCH)) return act(fetchNow);
    if (matches(e, REVIEW)) return act(toggleReview);
    if (reviewing && !typing) return reviewer?.key(e);
    if (e.key === "ArrowDown") return go(mod ? "last" : 1);
    if (e.key === "ArrowUp") return go(mod ? "first" : -1);
    if (e.key === "PageDown") return go(pageRows());
    if (e.key === "PageUp") return go(-pageRows());
    if (typing) {
      if (e.key === "Escape") act(() => input?.blur());
      return;
    }
    if (matches(e, ALL)) return act(() => (selected = new Set(rows.map((r) => r.id))));
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === " " && e.shiftKey) return act(() => markAbove());
    if (e.shiftKey) return;
    switch (e.key) {
      case "/":
        return act(focusSearch);
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
        return act(() => triage("shortlist"));
      case "x":
        return act(() => triage("dismiss"));
      case "m":
        return act(() => triage("read"));
      case "o":
        return act(() => openItems(targets()));
      case "a":
        return act(() => addItems(targets(), true));
      case "f":
        return act(() => find());
    }
  }

  /** The saved searches changed: kept, then their counts again. */
  async function keepSearches(list: api.SavedSearch[]) {
    try {
      await api.setSavedSearches(list);
      searches = list;
      await load(true);
    } catch (e) {
      say(String(e));
    }
  }

  /** A saved search shown: its choices, search and order in place of the tab's. */
  function applySearch(name: string) {
    const search = searches.find((s) => sameName(s.name, name));
    if (search) filters = applied(filters, search);
  }

  /** What the tab shows, as a saved search. */
  const current = (name: string): api.SavedSearch => ({ name, query: queryOf(filters), sort: filters.sort });

  function dropChip(chip: FeedChip) {
    filters = withoutChip(filters, chip);
  }

  /** Unfold a group of the sources in the sidebar, or fold it. */
  function fold(group: string, open: boolean) {
    const rest = filters.open.filter((g) => g !== group);
    filters.open = open ? [...rest, group] : rest;
  }

  let summary = $derived(
    `${rows.length} of ${counts?.all ?? 0}` + (selected.size > 1 ? ` · ${selected.size} selected` : ""),
  );
</script>

{#if subscribing}
  <SubscribeSheet
    {subscriptions}
    onsubscribed={(info) => {
      subscriptions = [...subscriptions, info];
      subscribed = true;
    }}
    onclose={closeSubscribe}
  />
{/if}

<main>
  {#if showSidebar}
    <aside class="sidebar" style:width="{sidebar}px">
      <FeedsSidebar
        {filters}
        {counts}
        {groups}
        onview={(view) => (filters.view = view)}
        onkind={(kind, toggle) => (filters = chooseKind(filters, kind, toggle))}
        oninterest={(name, toggle) => (filters = chooseInterest(filters, name, toggle))}
        onsource={(title, toggle) => (filters = chooseSource(filters, title, toggle))}
        onfold={fold}
      >
        {#snippet saved()}
          <SavedSearches
            searches={searches.map((s) => ({ name: s.name, count: counts?.searches?.[s.name] ?? 0 }))}
            shown={shownSearch}
            refusal={(name, own) => refusal(searches, name, own)}
            onapply={applySearch}
            onrename={(name, to) => keepSearches(renamed(searches, name, to))}
            onupdate={(name) => keepSearches(changed(searches, name, () => current(name)))}
            ondelete={(name) => keepSearches(removed(searches, name))}
          />
        {/snippet}
      </FeedsSidebar>
    </aside>
    <Splitter bind:width={sidebar} side="left" min={160} max={420} />
  {/if}
  <div class="right">
    <header>
      {@render tabs()}
      <div class="search">
        <input
          bind:this={input}
          bind:value={filters.query}
          type="search"
          placeholder="Search: words, a:, t:, abs:, j:, src:, kind:, prof:, score:3-, is:"
          spellcheck="false"
          autocomplete="off"
        />
        {#if filters.query}
          <button class="clear" aria-label="Clear the search" onclick={() => ((filters.query = ""), input?.focus())}>×</button>
        {/if}
      </div>
      <SaveSearch
        shown={shownSearch !== null}
        suggestion={chips.map((c) => c.text).join(" · ") || "All items"}
        refusal={(name) => refusal(searches, name)}
        onsave={(name) => keepSearches(added(searches, current(name)))}
      />
      <span class="notice">{notice ?? ""}</span>
      <span class="spacer"></span>
      <button class="tool" class:busy={fetching} disabled={fetching} title="Fetch every subscription ({label(FETCH)})" onclick={fetchNow}>
        <span class="spin">↻</span>
        {fetching ? "Fetching…" : "Fetch"}
      </button>
      <button class="tool" title="Subscribe to a journal, a preprint server, a news site or a blog" onclick={() => (subscribing = true)}>
        + Subscribe…
      </button>
      <button class="tool primary" class:on={reviewing} title="Review the starred items, one at a time ({label(REVIEW)})" onclick={toggleReview}>
        {reviewing ? "Leave review" : `Review starred (${shortlisted})`}
      </button>
    </header>
    <div class="content">
      {#if reviewing}
        <Review
          bind:this={reviewer}
          {colours}
          {pdfOf}
          onadd={add}
          ontriage={(id, t) => setTo([id], t)}
          onopen={(id) => openItems([id])}
          onitem={(id) => {
            toggleReview();
            showItem(id);
          }}
          {onentry}
          onclose={toggleReview}
        />
      {:else}
        <section class="list">
          <div class="filters">
            <span class="segment" role="group" aria-label="Kinds">
              {#each SEGMENTS as s (s.segment)}
                <button class:on={filters.segment === s.segment} onclick={() => (filters = chooseSegment(filters, s.segment))}>{s.name}</button>
              {/each}
            </span>
            {#each chips as chip (chip.kind)}
              <span class="chip">
                <span class="text" title={chip.text}>{chip.text}</span>
                <button aria-label="Remove the filter {chip.text}" onclick={() => dropChip(chip)}>×</button>
              </span>
            {/each}
            <span class="spacer"></span>
            <button class="sort" title="Mark the new items of the list as read: all, the older ones, those above" onclick={markMenu}>
              Mark read ▾
            </button>
            <button
              class="sort"
              title="Sort by score, by likeness to the library or by date"
              onclick={() => (filters.sort = nextSort(filters.sort))}
            >
              {summary} · {SORTS.find((s) => s.sort === filters.sort)?.text}
            </button>
          </div>
          {#if selected.size > 1}
            <div class="selbar">
              <span>{selected.size} selected</span>
              <button onclick={() => markSeen(chosen().filter((r) => r.triage === "new").map((r) => r.id))}>Mark read</button>
              <button onclick={() => setTo(chosen().filter((r) => r.triage === "seen").map((r) => r.id), "new")}>Mark unread</button>
              <button onclick={() => triage("shortlist", chosen().map((r) => r.id))}>★ Star</button>
              <button onclick={() => triage("dismiss", chosen().map((r) => r.id))}>Dismiss</button>
              <span class="spacer"></span>
              <button class="plain" onclick={() => (selected = new Set(focus ? [focus] : []))}>Clear selection</button>
            </div>
          {/if}
          {#if !loaded}
            <p class="empty">Loading the feeds…</p>
          {:else if failure && !rows.length}
            <p class="empty error">{failure}</p>
          {:else if !subscriptions.length && !rows.length}
            <p class="empty">
              No subscriptions yet. <button class="link" onclick={() => (subscribing = true)}>Subscribe…</button>
            </p>
          {:else if !rows.length}
            <p class="empty">No item matches.</p>
          {:else}
            <div class="rows">
            <FeedTable
              {rows}
              {selected}
              {focus}
              sort={filters.sort}
              widths={columns}
              {colours}
              onsort={(sort) => (filters.sort = sort)}
              onwidths={(widths) => (columns = widths)}
              onstar={(id) => triage("shortlist", [id])}
              onselect={select}
              onopen={(id) => openItems(selected.has(id) ? targets() : [id])}
              onmenu={rowMenu}
            />
            {#if reading && !beside && focusRow}
              <div class="reader">
                <button class="pinned" title="Back to the list (Esc)" onclick={() => (reading = false)}>
                  <span class="caret">▾</span>
                  <span class="title"><Marked spans={focusRow.title} /></span>
                  <span class="muted">{focusRow.journal ?? focusRow.sources[0] ?? ""}</span>
                  <span class="muted keys">↑ ↓ previous, next · Esc the list</span>
                </button>
                <div class="page">
                  <div class="column">
                    {#if detail}{@render itemDetails(detail, true)}{/if}
                  </div>
                </div>
              </div>
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
              <p class="empty">No item selected</p>
            {/if}
          </aside>
        {/if}
      {/if}
    </div>
    <footer class="status">
      <span>
        {fetching ? "Fetching…" : fetchLine(last, now)}
        {#if !fetching && last?.failed.length}
          <button
            class="failed"
            title={last.failed.map((f) => `${f.title}: ${f.message}`).join("\n")}
            onclick={() => (failures = !failures)}>{last.failed.length} failed</button
          >
        {/if}
      </span>
      <span class="spacer"></span>
      <span>
        {subscriptions.length} {subscriptions.length === 1 ? "subscription" : "subscriptions"} · Starred {shortlisted} · Added
        {counts?.is.added ?? 0}
      </span>
      {#if failures && last?.failed.length}
        <button class="backdrop" aria-label="Close" onclick={() => (failures = false)}></button>
        <div class="failures" role="dialog" aria-label="Failed subscriptions">
          {#each last.failed as f (f.subscription)}
            <p><b>{f.title}</b> {f.message}</p>
          {/each}
        </div>
      {/if}
    </footer>
  </div>
</main>

{#snippet itemDetails(d: FeedDetail, wide: boolean)}
  <FeedDetails
    detail={d}
    {wide}
    count={focus && selected.has(focus) ? selected.size : 1}
    {byId}
    {colours}
    finding={finding === d.id}
    {pdfOf}
    onadd={(pdf) => addItems(targets().length ? targets() : [d.id], pdf)}
    onaddlink={(link: Linked) => add({ id: link.doi, item: link.item }, true)}
    ontriage={(action) => triage(action, targets().length ? targets() : [d.id])}
    onshortlistlink={(item) => triage("shortlist", [item])}
    onopen={() => openItems(targets().length ? targets() : [d.id])}
    onfind={() => find(d.id)}
    onitem={showItem}
    {onentry}
  />
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
  /* the table, and the item read over it: the table stays as it was, scrolled where it was */
  .rows {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
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
    background: color-mix(in srgb, var(--feeds) 10%, transparent);
    color: var(--fg);
    font: inherit;
    text-align: left;
  }
  .pinned .caret {
    color: var(--feeds);
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
  .tool.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
  .tool.primary.on {
    border-color: var(--border);
    background: var(--line);
    color: var(--fg);
  }
  .tool.busy .spin {
    animation: spin 1s linear infinite;
  }
  .spin {
    display: inline-block;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* Papers | Posts | All, the choices as chips, the count and the sort */
  .filters {
    flex: none;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 6px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--line);
    font-size: 12px;
  }
  .segment {
    display: inline-flex;
    margin-right: 6px;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .segment button {
    padding: 2px 10px;
    border: none;
    background: none;
    color: var(--muted);
    font: inherit;
  }
  .segment button + button {
    border-left: 1px solid var(--border);
  }
  .segment button.on {
    background: var(--line);
    color: var(--fg);
    font-weight: 600;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    max-width: 260px;
    padding: 1px 2px 1px 8px;
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--fg);
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
  .sort {
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
    font: inherit;
    font-variant-numeric: tabular-nums;
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

  /* the last fetch, and what the feeds hold */
  .status {
    position: relative;
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
  .failed {
    margin-left: 4px;
    padding: 0;
    border: none;
    background: none;
    color: var(--error);
    font: inherit;
    text-decoration: underline;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 2;
    border: none;
    background: none;
  }
  .failures {
    position: absolute;
    bottom: calc(100% + 4px);
    left: 12px;
    z-index: 3;
    max-width: min(560px, 80vw);
    max-height: 50vh;
    overflow-y: auto;
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--menu-bg);
    color: var(--fg);
    white-space: normal;
    -webkit-user-select: text;
    user-select: text;
  }
  .failures p {
    margin: 0 0 4px;
    overflow-wrap: anywhere;
  }

  .empty {
    margin: auto;
    padding: 24px;
    color: var(--muted);
    text-align: center;
  }
  .empty.error {
    color: var(--error);
  }
  button.link {
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }
</style>
