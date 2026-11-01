<script lang="ts">
  // The Feeds tab of the main window (docs/design.md, Feeds; the mockup docs/mockups/feeds.html): the sidebar (the
  // views, kinds, interests and sources), the list of the items its choices and the search select and the details
  // of the one selected (ItemPane, on an ItemList: beside the list or in place of it, the setting `feeds_beside`);
  // the keys of the triage; the Review of the starred items, over the list and the details; the Subscribe sheet.
  // The window (src/routes/library/+page.svelte) keeps the
  // layout, and passes on the commands of the menu and the keys while the tab is in front. The Add sheet is the
  // library's: it opens over this tab, which stays in front after the paper is added.

  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu } from "@tauri-apps/api/menu";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { onDestroy, onMount, untrack, type Snippet } from "svelte";
  import { label, matches, type Shortcut } from "$lib/keys";
  import { modalOpen } from "$lib/library/modal.svelte";
  import Splitter from "$lib/library/Splitter.svelte";
  import { ItemList, typing } from "$lib/list/ItemList.svelte";
  import ItemPane from "$lib/list/ItemPane.svelte";
  import "$lib/list/tab.css";
  import { added, changed, refusal, removed, renamed, sameName } from "$lib/saved";
  import SavedSearches from "$lib/SavedSearches.svelte";
  import SaveSearch from "$lib/SaveSearch.svelte";
  import * as api from "./api";
  import type { FeedCounts, FeedDetail, FeedRow, FetchSummary, Linked, SubscriptionInfo, Triage } from "./api";
  import FeedsSidebar from "./FeedsSidebar.svelte";
  import type { FeedColumn } from "./FeedTable.svelte";
  import Review from "./Review.svelte";
  import SubscribeSheet from "./SubscribeSheet.svelte";
  import {
    SOURCE_KINDS,
    SORTS,
    applied,
    chooseInterest,
    chooseKind,
    chooseSource,
    feedChips,
    interestColours,
    isPost,
    nextSort,
    isShown,
    queryOf,
    requestOf,
    sourceGroups,
    withoutChip,
    type FeedChip,
    type FeedFilters,
  } from "./filters";
  import {
    fetchLine,
    mergeRows,
    newAbove,
    newOlderThan,
    snoozeUntil,
    stale,
    toggled,
    type TriageAction,
  } from "./items";
  import { Dwell } from "./review";

  let {
    tabs,
    filters = $bindable(),
    columns = $bindable(),
    shown = $bindable(),
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
    /** when the list was last brought up to date: what a fetch in the background brought since waits behind
     *  *N new · Show*; a fetch asked for here (⌘R, Fetch, a subscription) shows it at once. The layout keeps it. */
    shown: string | null;
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


  const REFRESH: Shortcut = { key: "r", mod: true };
  const FETCH: Shortcut = { key: "r", mod: true, shift: true };
  const REVIEW: Shortcut = { key: "Enter", mod: true };

  let counts = $state<FeedCounts | null>(null);
  let subscriptions = $state<SubscriptionInfo[]>([]);
  /** the first answer came */
  let loaded = $state(false);
  let failure = $state<string | null>(null);
  /** the post whose papers are being looked for */
  let finding = $state<string | null>(null);
  let last = $state<FetchSummary | null>(null);
  /** a fetch reads the feeds; then looks up what they brought */
  let fetching = $state(false);
  let lookingUp = $state(false);
  /** the failures of the last fetch, listed */
  let failures = $state(false);
  /** the Review of the shortlist, over the list and the details */
  let reviewing = $state(false);
  let reviewer = $state<ReturnType<typeof Review>>();
  /** the Subscribe sheet, open; whether a subscription was made in it (fetched when it closes) */
  let subscribing = $state(false);
  let subscribed = false;
  let now = $state(Date.now());
  /** above this many, Mark read asks first */
  const MARK_ASK = 50;
  /** the snoozes of Later ▾ and the row menu: till the start of the day */
  const SNOOZES: [string, number][] = [
    ["1 Day", 1],
    ["3 Days", 3],
    ["1 Week", 7],
    ["1 Month", 30],
  ];

  let request = $derived(requestOf(filters));
  /** the items the filters select; those triaged here stay in place until the filters change */
  const list = new ItemList({
    store: "feeds",
    query: () => request.query,
    onadd: (initial) => onadd(initial),
    reload: () => load(true),
    // an item opened is seen
    opened: (ids) => {
      const fresh = ids.filter((id) => list.byId.get(id)?.triage === "new");
      if (fresh.length) list.setTo(fresh, "seen");
    },
  });
  let chips = $derived(feedChips(filters));
  let groups = $derived(sourceGroups(subscriptions, counts));
  let colours = $derived(interestColours(Object.keys(counts?.total.interests ?? {})));
  let shortlisted = $derived(counts?.is.shortlisted ?? 0);
  /** the saved searches of the sidebar; the one the tab shows */
  let searches = $state<api.SavedSearch[]>([]);
  /** how many days the feeds keep an item (the setting): what *All* reaches back */
  let keepDays = $state<number | null>(null);
  let shownSearch = $derived(searches.find((s) => isShown(filters, s))?.name ?? null);

  /** An item becomes seen once it has stayed selected a moment (`Dwell`); one passed over stays new. */
  const dwell = new Dwell((id) => {
    if (list.byId.get(id)?.triage === "new") list.setTo([id], "seen");
  });

  onMount(() => {
    const unlisten = [
      // a fetch in the background (polling) or from here
      listen<api.FetchPhase>(api.EVENTS.fetching, (e) => {
        fetching = e.payload === "reading";
        lookingUp = e.payload === "looking-up";
        if (!fetching) refresh();
      }),
      // a fetch, a triage from elsewhere, papers found
      listen(api.EVENTS.changed, () => {
        list.version++;
        load(true);
      }),
      // back from the settings: the days kept again
      getCurrentWindow().onFocusChanged(({ payload: focused }) => {
        if (focused) readKeepDays();
      }),
    ];
    const clock = setInterval(() => (now = Date.now()), 30000);
    start();
    readKeepDays();
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
    if (stale(last) && subscriptions.length) fetchNow(false);
  }

  async function readKeepDays() {
    try {
      keepDays = (await api.settings()).keepDays;
    } catch {
      // *All* says no days
    }
  }

  /** ⇧⌘R, ↻: every subscription fetched, or the one of id `only`; the list again once the feeds are read (the
   *  lookups that follow go on, the list again when they end). */
  async function fetchNow(show = true, only?: string) {
    if (fetching) return;
    fetching = true;
    failures = false;
    try {
      const summary = await api.fetchFeeds(only);
      const failed = summary.failed.length;
      if (failed) list.say(`${failed} ${failed === 1 ? "subscription" : "subscriptions"} failed`);
    } catch (e) {
      list.say(String(e));
    } finally {
      fetching = false;
    }
    if (show) shown = new Date().toISOString();
    await refresh();
  }

  /** N new · Show: what was fetched since the list was shown, into it. */
  function showHeld() {
    shown = new Date().toISOString();
    load(true);
  }

  /** ⌘R: the list brought up to date, nothing fetched: what was held back in, the items triaged here gone from
   *  where they no longer belong, sorted again. */
  function refreshList() {
    list.sticky.clear();
    shown = new Date().toISOString();
    load(false);
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
  /** the items fetched since the list was shown, held back */
  let held = $state(0);

  /** The items the filters select, and the counts; with `keep`, the items triaged here stay in their place. */
  async function load(keep: boolean) {
    const n = ++asked;
    try {
      shown ??= new Date().toISOString();
      const answer = await api.view({ ...request, shown });
      if (n !== asked) return;
      list.rows = keep ? mergeRows(list.rows, answer.rows, list.sticky) : answer.rows;
      counts = answer.counts;
      held = answer.held;
      failure = null;
    } catch (e) {
      if (n !== asked) return;
      failure = String(e);
    }
    loaded = true;
    list.selection.prune(list.ids, keep);
  }

  // the filters changed: the list again, the items triaged before let go
  $effect(() => {
    request;
    untrack(() => {
      list.sticky.clear();
      load(false);
    });
  });

  // the item in focus, if new, is seen once it stays
  $effect(() => {
    const id = list.selection.focus;
    // a reminder stays new until marked read
    untrack(() => {
      const row = id === null ? undefined : list.byId.get(id);
      dwell.show(id, row?.triage === "new" && !row.reminder);
    });
  });

  /** s, x, m: the triage of the items selected toggled (`toggled`); an item not in the list (a linked paper) is
   *  asked for. */
  async function triage(action: TriageAction, ids = list.targets()) {
    const found = await Promise.all(ids.map((id) => list.byId.get(id) ?? list.detailOf(id)));
    const items = found.filter((r): r is FeedRow | FeedDetail => !!r);
    const change = toggled(items, action);
    if (change) list.setTo(change.ids, change.triage);
  }

  /** ⇧Space, the menu: the new items above the one in focus (the menu's row) marked seen. */
  function markAbove(from = list.selection.focus) {
    const ids = newAbove(list.rows, from);
    if (!ids.length) return;
    list.setTo(ids, "seen");
    list.say(`Marked ${ids.length} as seen`);
  }

  /** Mark read ▾: the new items of the list, those older than some days, those above the selection, marked seen
   *  (above 50, after asking). */
  async function markMenu() {
    const item = (text: string, ids: string[]) =>
      MenuItem.new({ text: `${text} (${ids.length})`, enabled: ids.length > 0, action: () => markSeen(ids) });
    const all = list.rows.filter((r) => r.triage === "new").map((r) => r.id);
    const older = (days: number) => newOlderThan(list.rows, days, Date.now());
    const menu = await Menu.new({
      items: [
        await item("All in This List", all),
        await PredefinedMenuItem.new({ item: "Separator" }),
        await item("Older than 1 Day", older(1)),
        await item("Older than 3 Days", older(3)),
        await item("Older than 1 Week", older(7)),
        await PredefinedMenuItem.new({ item: "Separator" }),
        await item("All Above the Selection", newAbove(list.rows, list.selection.focus)),
      ],
    });
    await menu.popup();
  }

  async function markSeen(ids: string[]) {
    const question = `Mark ${ids.length} items as read?`;
    if (ids.length > MARK_ASK && !(await ask(question, { title: "Mark read", okLabel: "Mark read", cancelLabel: "Cancel" }))) return;
    list.setTo(ids, "seen");
    list.say(`Marked ${ids.length} as read`);
  }

  /** U: kept unread (a reminder due now, first in the list); the reminders of items all kept so, cleared. */
  function keepUnread(ids = list.targets()) {
    const due = ids.every((id) => list.byId.get(id)?.reminder === "due");
    if (due) return list.setTo(ids, "new");
    remindOf(ids, null);
  }

  /** Items new again, to be reminded of in some days (snoozed till then), else now (kept unread). */
  async function remindOf(ids: string[], days: number | null) {
    if (!ids.length) return;
    const until = days === null ? null : snoozeUntil(days, new Date());
    const set = new Set(ids);
    for (const id of ids) list.sticky.add(id);
    list.rows = list.rows.map((r) =>
      set.has(r.id) ? { ...r, triage: "new", reminder: until ? "snoozed" : "due", remind: until?.toISOString() ?? null } : r,
    );
    try {
      await api.remind(ids, until?.toISOString() ?? null);
      const n = ids.length === 1 ? "" : ` ${ids.length}`;
      list.say(until ? `Snoozed${n} until ${until.toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" })}` : `Kept${n} unread`);
    } catch (e) {
      list.say(String(e));
    }
    await load(true);
  }

  /** Z, Later ▾: kept unread, snoozed for some days, or the reminder cleared. */
  async function laterMenu(ids = list.targets()) {
    if (!ids.length) return;
    const item = (text: string, action: () => void, enabled = true) => MenuItem.new({ text, action, enabled });
    const reminded = ids.some((id) => list.byId.get(id)?.reminder);
    const menu = await Menu.new({
      items: [
        await item("Keep Unread", () => remindOf(ids, null)),
        await PredefinedMenuItem.new({ item: "Separator" }),
        ...(await Promise.all(SNOOZES.map(([text, days]) => item(`Snooze ${text}`, () => remindOf(ids, days))))),
        await PredefinedMenuItem.new({ item: "Separator" }),
        await item("Clear Reminder", () => list.setTo(ids, "new"), reminded),
      ],
    });
    await menu.popup();
  }

  /** f: the papers behind a post looked for again (its page, then Crossref by its title). */
  async function find(id = list.selection.focus) {
    if (!id || finding) return;
    const d = await list.detailOf(id);
    if (!d || !isPost(d.kind)) return list.say("F finds the paper behind a post");
    finding = id;
    try {
      const found = await api.findPapers(id);
      list.say(found.length ? `Found ${found.length} ${found.length === 1 ? "paper" : "papers"}` : "No paper found");
      list.version++;
      await load(true);
    } catch (e) {
      list.say(String(e));
    } finally {
      finding = null;
    }
  }

  /** Right-click on a row: the items selected when it is one of them, else that item alone. */
  async function rowMenu(id: string) {
    if (!list.selection.selected.has(id)) list.showItem(id);
    const ids = list.targets();
    const items = ids.map((i) => list.byId.get(i)).filter((r): r is FeedRow => r !== undefined);
    if (!items.length) return;
    const every = (t: Triage) => items.every((r) => r.triage === t);
    const item = (text: string, action: () => void, enabled = true) => MenuItem.new({ text, action, enabled });
    const separator = () => PredefinedMenuItem.new({ item: "Separator" });
    const posts = items.length === 1 && isPost(items[0].kind);
    // one item: the kind of each subscription that lists it, to set right
    const sources = items.length === 1 ? ((await list.detailOf(id))?.sources ?? []) : [];
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
        await item(posts ? "Open Post" : "Open", () => list.openItems(ids)),
        await item("Add…", () => list.addItems(ids, true)),
        await item("Add without PDF…", () => list.addItems(ids, false)),
        ...(posts ? [await item("Find Linked Paper", () => find(id))] : []),
        await separator(),
        await item(every("shortlisted") ? "Unstar" : "Star", () => triage("shortlist", ids)),
        await item(items.some((r) => r.triage === "new") ? "Mark as Seen" : "Mark as New", () => triage("read", ids)),
        await item(every("dismissed") ? "Restore" : "Dismiss", () => triage("dismiss", ids)),
        await item("Keep Unread", () => remindOf(ids, null)),
        await Submenu.new({
          text: "Snooze",
          items: await Promise.all(SNOOZES.map(([text, days]) => item(text, () => remindOf(ids, days)))),
        }),
        await item("Clear Reminder", () => list.setTo(ids, "new"), items.some((r) => r.reminder)),
        await separator(),
        await item("Mark All Above as Seen", () => markAbove(id), newAbove(list.rows, id).length > 0),
        ...(sources.length ? [await separator(), ...(await Promise.all(sources.map(kindOf)))] : []),
      ],
    });
    await menu.popup();
  }

  /** Right-click on a source of the sidebar (the subscriptions of that title): its kind, a fetch of it alone, the
   *  end of it. */
  async function sourceMenu(title: string) {
    const subs = subscriptions.filter((s) => s.title === title);
    if (!subs.length) return;
    const feeds = subs.filter((s) => "rss" in s.feed);
    const menu = await Menu.new({
      items: [
        ...(feeds.length
          ? [
              await Submenu.new({
                text: "Source Is",
                items: await Promise.all(
                  SOURCE_KINDS.map((k) =>
                    CheckMenuItem.new({
                      text: k.name,
                      checked: feeds.every((s) => s.kind === k.kind),
                      action: async () => {
                        for (const s of feeds) await setKind(s.id, s.title, k.kind);
                      },
                    }),
                  ),
                ),
              }),
            ]
          : []),
        await MenuItem.new({
          text: "Fetch This Source",
          enabled: !fetching,
          action: async () => {
            for (const s of subs) await fetchNow(true, s.id);
          },
        }),
        await PredefinedMenuItem.new({ item: "Separator" }),
        await MenuItem.new({ text: "Unsubscribe…", action: () => unsubscribe(title, subs) }),
      ],
    });
    await menu.popup();
  }

  /** The subscriptions of a source ended, once asked: their items stay until they are old. */
  async function unsubscribe(title: string, subs: SubscriptionInfo[]) {
    const sure = await ask(`Its items stay until they are older than the days the settings keep them.`, {
      title: `Unsubscribe from ${title}?`,
      kind: "warning",
      okLabel: "Unsubscribe",
    });
    if (!sure) return;
    try {
      for (const s of subs) {
        await api.unsubscribe(s.id);
        unsubscribed(s.id);
      }
      list.say(`Unsubscribed from ${title}`);
    } catch (e) {
      list.say(String(e));
    }
    load(true);
  }

  /** A subscription ended, here or in the Subscribe sheet. */
  function unsubscribed(id: string) {
    const gone = subscriptions.find((s) => s.id === id);
    subscriptions = subscriptions.filter((s) => s.id !== id);
    if (gone && !subscriptions.some((s) => s.title === gone.title)) {
      filters.sources = filters.sources.filter((t) => t !== gone.title);
    }
  }

  /** A subscription set to another kind, from the row menu: its items move at once. */
  async function setKind(id: string, title: string, kind: api.SourceKind) {
    try {
      await api.updateSubscription(id, { kind });
      list.say(`${title}: ${SOURCE_KINDS.find((k) => k.kind === kind)?.name}`);
      list.version++;
      await load(true);
    } catch (e) {
      list.say(String(e));
    }
  }

  /** The Subscribe sheet closed: what was subscribed to is fetched. */
  function closeSubscribe() {
    subscribing = false;
    if (subscribed) fetchNow();
    else load(true);
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
    list.focusSearch();
  }

  /** A key of the window while the tab is in front (none while a sheet is open): ⌘R brings the list up to date,
   *  ⇧⌘R fetches, ⌘↩ starts the Review (which then has the keys); the list's keys move and read (`navKey`); s, x,
   *  m, u (keep unread), z (later), o, a, f act on the selection; ⇧Space marks all above as seen. */
  export function key(e: KeyboardEvent) {
    if (modalOpen()) return;
    const act = (action: () => unknown) => {
      e.preventDefault();
      action();
    };
    if (matches(e, REFRESH)) return act(refreshList);
    if (matches(e, FETCH)) return act(() => fetchNow());
    if (matches(e, REVIEW)) return act(toggleReview);
    if (reviewing && !typing(e)) return reviewer?.key(e);
    if (list.navKey(e) || typing(e) || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === " " && e.shiftKey) return act(() => markAbove());
    if (e.shiftKey) return;
    switch (e.key) {
      case "s":
        return act(() => triage("shortlist"));
      case "x":
        return act(() => triage("dismiss"));
      case "m":
        return act(() => triage("read"));
      case "o":
        return act(() => list.openItems(list.targets()));
      case "a":
        return act(() => list.addItems(list.targets(), true));
      case "f":
        return act(() => find());
      case "u":
        return act(() => keepUnread());
      case "z":
        return act(() => laterMenu());
    }
  }

  /** The saved searches changed: kept, then their counts again. */
  async function keepSearches(saved: api.SavedSearch[]) {
    try {
      await api.setSavedSearches(saved);
      searches = saved;
      await load(true);
    } catch (e) {
      list.say(String(e));
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
    `${list.rows.length} of ${counts?.all ?? 0}` + (list.selection.selected.size > 1 ? ` · ${list.selection.selected.size} selected` : ""),
  );
</script>

{#if subscribing}
  <SubscribeSheet
    {subscriptions}
    onsubscribed={(info) => {
      subscriptions = [...subscriptions, info];
      subscribed = true;
    }}
    onunsubscribed={unsubscribed}
    onclose={closeSubscribe}
  />
{/if}

<main class="tab" style:--tint="var(--feeds)">
  {#if showSidebar}
    <aside class="sidebar" style:width="{sidebar}px">
      <FeedsSidebar
        {filters}
        {counts}
        {groups}
        {keepDays}
        onview={(view) => (filters.view = view)}
        onkind={(kind, toggle) => (filters = chooseKind(filters, kind, toggle))}
        oninterest={(name, toggle) => (filters = chooseInterest(filters, name, toggle))}
        onsource={(title, toggle) => (filters = chooseSource(filters, title, toggle))}
        onsourcemenu={sourceMenu}
        onsubscribe={() => (subscribing = true)}
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
          bind:this={list.input}
          bind:value={filters.query}
          type="search"
          placeholder="Search: words, a:, t:, abs:, j:, src:, kind:, prof:, score:3-, is:"
          spellcheck="false"
          autocomplete="off"
        />
        {#if filters.query}
          <button class="clear" aria-label="Clear the search" onclick={() => ((filters.query = ""), list.input?.focus())}>×</button>
        {/if}
      </div>
      <SaveSearch
        shown={shownSearch !== null}
        suggestion={chips.map((c) => c.text).join(" · ") || "All items"}
        refusal={(name) => refusal(searches, name)}
        onsave={(name) => keepSearches(added(searches, current(name)))}
      />
      <span class="notice">{list.notice ?? ""}</span>
      <span class="spacer"></span>
      {#if held}
        <button class="tool held" title="Show what was fetched since the list was brought up to date ({label(REFRESH)} also sorts it again)" onclick={showHeld}>
          {held} new · Show
        </button>
      {/if}
      <button class="tool" class:busy={fetching} disabled={fetching} title="Fetch every subscription ({label(FETCH)})" onclick={() => fetchNow()}>
        <span class="spin">↻</span>
        {fetching ? "Fetching…" : "Fetch"}
      </button>
      <button class="tool" title="Subscribe to a journal, a preprint server, a news site or a blog, or unsubscribe" onclick={() => (subscribing = true)}>
        Subscribe…
      </button>
      <button class="tool primary" class:on={reviewing} title="Review the starred items, one at a time ({label(REVIEW)})" onclick={toggleReview}>
        {reviewing ? "Leave review" : `Review starred (${shortlisted})`}
      </button>
    </header>
    {#if reviewing}
      <Review
        bind:this={reviewer}
        {colours}
        pdfOf={list.pdfOf}
        onadd={list.add}
        ontriage={(id, t) => list.setTo([id], t)}
        onopen={(id) => list.openItems([id])}
        onitem={(id) => {
          toggleReview();
          list.showItem(id);
        }}
        {onentry}
        onclose={toggleReview}
      />
    {:else}
      <ItemPane
        {list}
        table={{
          sort: filters.sort,
          widths: columns,
          onsort: (sort) => (filters.sort = sort),
          onwidths: (widths) => (columns = widths),
          onstar: (id) => triage("shortlist", [id]),
          onmenu: rowMenu,
        }}
        {colours}
        none="No item selected"
        bind:details
        {showDetails}
        bar={listBar}
        empty={loaded && list.rows.length ? undefined : noRows}
        actions={itemActions}
        finding={finding !== null && finding === list.detail?.id}
        onaddlink={(link: Linked) => list.add({ id: link.doi, item: link.item }, true)}
        onshortlistlink={(item) => triage("shortlist", [item])}
        onfind={find}
        {onentry}
      />
    {/if}
    <footer class="status">
      <span>
        {fetching ? "Fetching…" : lookingUp ? "Looking up in Crossref and OpenAlex…" : fetchLine(last, now)}
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

<!-- above the list: the choices as chips, Mark read ▾, the count and the sort; the selection bar -->
{#snippet listBar()}
  <div class="filters">
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
    <button class="sort" title="Sort by score, by likeness to the library or by date" onclick={() => (filters.sort = nextSort(filters.sort))}>
      {summary} · {SORTS.find((s) => s.sort === filters.sort)?.text}
    </button>
  </div>
  {#if list.selection.selected.size > 1}
    <div class="selbar">
      <span>{list.selection.selected.size} selected</span>
      <button onclick={() => markSeen(list.chosen().filter((id) => list.byId.get(id)?.triage === "new"))}>Mark read</button>
      <button onclick={() => list.setTo(list.chosen().filter((id) => list.byId.get(id)?.triage === "seen"), "new")}>Mark unread</button>
      <button onclick={() => triage("shortlist", list.chosen())}>★ Star</button>
      <button onclick={() => triage("dismiss", list.chosen())}>Dismiss</button>
      <button onclick={() => laterMenu(list.chosen())}>⏰ Later ▾</button>
      <span class="spacer"></span>
      <button class="plain" onclick={() => list.selection.only(list.selection.focus)}>Clear selection</button>
    </div>
  {/if}
{/snippet}

<!-- in place of the list: loading, failed, no subscription, nothing matches -->
{#snippet noRows()}
  {#if !loaded}
    <p class="empty">Loading the feeds…</p>
  {:else if failure}
    <p class="empty error">{failure}</p>
  {:else if !subscriptions.length}
    <p class="empty">
      No subscriptions yet. <button class="link" onclick={() => (subscribing = true)}>Subscribe…</button>
    </p>
  {:else}
    <p class="empty">No item matches.</p>
  {/if}
{/snippet}

<!-- the buttons of the details: a post's, a paper's -->
{#snippet itemActions(d: FeedDetail)}
  {#if isPost(d.kind)}
    <button class="open" onclick={() => list.openItems(list.targetsOr(d.id))} disabled={!d.url}>Open post ↗ <kbd>O</kbd></button>
    <button onclick={() => triage("read", list.targetsOr(d.id))}>{d.triage === "new" ? "Mark read" : "Mark unread"} <kbd>M</kbd></button>
    <button onclick={() => triage("shortlist", list.targetsOr(d.id))}>{d.triage === "shortlisted" ? "★ Starred" : "☆ Star"} <kbd>S</kbd></button>
    <button onclick={() => triage("dismiss", list.targetsOr(d.id))}>{d.triage === "dismissed" ? "Dismissed" : "Dismiss"} <kbd>X</kbd></button>
    <button onclick={() => laterMenu(list.targetsOr(d.id))} title="Keep it unread (U), or snooze it">⏰ Later <kbd>Z</kbd></button>
  {:else}
    <button class="primary" onclick={() => list.addItems(list.targetsOr(d.id), true)} title="Add to the library, with the free PDF if there is one">Add <kbd>A</kbd></button>
    <button class="open" onclick={() => list.openItems(list.targetsOr(d.id))} disabled={!d.url && !d.doi} title="Open its page">Open ↗ <kbd>O</kbd></button>
    <button onclick={() => list.addItems(list.targetsOr(d.id), false)} title="Add to the library without looking for its PDF">Add without PDF</button>
    <button onclick={() => triage("shortlist", list.targetsOr(d.id))}>{d.triage === "shortlisted" ? "★ Starred" : "☆ Star"} <kbd>S</kbd></button>
    <button onclick={() => triage("dismiss", list.targetsOr(d.id))}>{d.triage === "dismissed" ? "Dismissed" : "Dismiss"} <kbd>X</kbd></button>
    <button onclick={() => laterMenu(list.targetsOr(d.id))} title="Keep it unread (U), or snooze it">⏰ Later <kbd>Z</kbd></button>
  {/if}
{/snippet}

<style>
  /* what a fetch in the background brought, waiting */
  .tool.held {
    border-color: color-mix(in srgb, var(--feeds) 40%, transparent);
    background: color-mix(in srgb, var(--feeds) 14%, transparent);
    color: var(--feeds);
    font-weight: 500;
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
  .chip {
    max-width: 260px;
  }

  /* the failures of the last fetch */
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

  button.link {
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }
</style>
