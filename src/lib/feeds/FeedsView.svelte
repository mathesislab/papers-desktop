<script lang="ts">
  // The Feeds tab of the main window (docs/design.md, Feeds; the mockup docs/mockups/feeds.html): the sidebar (the
  // views, kinds, interests and sources), the list of the items its choices and the search select and the details
  // of the one selected (ItemPane, on an ItemList: beside the list or in place of it, as the window's switch says);
  // the keys of the triage; the Review of the starred items, over the list and the details; the Subscribe sheet.
  // The window (src/routes/main/+page.svelte) keeps the layout, gives what both its Feeds and Search tabs take
  // (`tabContext`), and passes on the commands of the menu and the keys while the tab is in front. The Add sheet is
  // the library's: it opens over this tab, which stays in front after the paper is added.

  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu } from "@tauri-apps/api/menu";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { onDestroy, onMount, untrack } from "svelte";
  import { label, matches, type Shortcut } from "$lib/keys";
  import { modalOpen } from "$lib/library/modal.svelte";
  import Splitter from "$lib/library/Splitter.svelte";
  import { ItemList, typing } from "$lib/list/ItemList.svelte";
  import ItemPane from "$lib/list/ItemPane.svelte";
  import "$lib/list/tab.css";
  import { tabContext } from "$lib/list/tab";
  import { added, changed, refusal, removed, renamed, sameName } from "$lib/saved";
  import SavedSearches from "$lib/SavedSearches.svelte";
  import SaveSearch from "$lib/SaveSearch.svelte";
  import { SNOOZES, buttonText, feedActions, titled, type Action, type ActionName, type Item } from "./actions";
  import * as api from "./api";
  import type { FeedCounts, FeedDetail, FeedRow, FetchSummary, Linked, SubscriptionInfo } from "./api";
  import FeedsSidebar from "./FeedsSidebar.svelte";
  import { FEED_ORDER, FEED_SPECS, type FeedColumn } from "./columns";
  import type { Shown } from "$lib/list/columns";
  import Review from "./Review.svelte";
  import SubscribeSheet from "./SubscribeSheet.svelte";
  import {
    SORTS,
    applied,
    feedChips,
    nextSort,
    isShown,
    queryOf,
    requestOf,
    withoutChip,
    type FeedChip,
    type FeedFilters,
  } from "./filters";
  import {
    Dwell,
    fetchLine,
    mergeRows,
    newAbove,
    newOlderThan,
    snoozeUntil,
    stale,
    toggled,
    type TriageAction,
  } from "./items";
  import { SOURCE_KINDS, isPost, sourceKinds } from "./kinds";
  import { sourceGroups } from "./subscriptions";

  let {
    filters = $bindable(),
    columns = $bindable(),
    shown = $bindable(),
  }: {
    /** the choices of the tab, its search and sort: the window's layout keeps them */
    filters: FeedFilters;
    /** the list's columns, in their order, with their widths: the window's layout keeps them */
    columns: Shown<FeedColumn>[];
    /** when the list was last brought up to date: what a fetch in the background brought since waits behind
     *  *N new · Show*; a fetch asked for here (⌘R, Fetch, a subscription) shows it at once. The layout keeps it. */
    shown: string | null;
  } = $props();

  const tab = tabContext();


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
  /** the Review of the starred items, over the list and the details */
  let reviewing = $state(false);
  let reviewer = $state<ReturnType<typeof Review>>();
  /** the Subscribe sheet, open; whether a subscription was made in it (fetched when it closes) */
  let subscribing = $state(false);
  let subscribed = false;
  let now = $state(Date.now());
  /** above this many, Mark read asks first */
  const MARK_ASK = 50;
  /** the buttons of the details, a post's and a paper's; those of the selection bar */
  const POST_BUTTONS: ActionName[] = ["open", "read", "star", "dismiss", "later"];
  const PAPER_BUTTONS: ActionName[] = ["add", "open", "addBare", "star", "dismiss", "later"];
  const BAR_BUTTONS: ActionName[] = ["read", "star", "dismiss", "later"];

  let request = $derived(requestOf(filters));
  /** the items the filters select; those triaged here stay in place until the filters change */
  const list = new ItemList({
    store: "feeds",
    query: () => request.query,
    reload: () => load(true),
    // an item opened is seen
    opened: (ids) => {
      const fresh = ids.filter((id) => list.byId.get(id)?.triage === "new");
      if (fresh.length) list.setTo(fresh, "seen");
    },
  });
  let chips = $derived(feedChips(filters));
  let groups = $derived(sourceGroups(subscriptions, counts));
  let starred = $derived(counts?.is.starred ?? 0);
  /** the saved views of the sidebar; the one the tab shows */
  let views = $state<api.SavedView[]>([]);
  /** how many days the feeds keep an item (the setting): what *All* reaches back */
  let keepDays = $state<number | null>(null);
  let shownView = $derived(views.find((v) => isShown(filters, v))?.name ?? null);

  /** what can be done to the items: the keys, the menus, the buttons */
  const actions = feedActions({
    open: (ids) => list.openItems(ids),
    add: (ids, pdf) => list.addItems(ids, pdf),
    find: (id) => find(id),
    focus: () => list.selection.focus,
    triage,
    keepUnread,
    later: laterMenu,
    clearReminder: (ids) => list.setTo(ids, "new"),
  });

  /** An item becomes seen once it has stayed selected a moment (`Dwell`); one passed over stays new. */
  const dwell = new Dwell((id) => {
    if (list.byId.get(id)?.triage === "new") list.setTo([id], "seen");
  });

  onMount(() => {
    const unlisten = [
      // where a fetch is, in the background (polling) or from here; the feeds read, the summary and the
      // subscriptions again
      listen<api.FetchPhase>(api.EVENTS.fetching, (e) => {
        fetching = e.payload === "reading";
        lookingUp = e.payload === "looking-up";
        if (!fetching) readFetch();
      }),
      // what changed the items, here or elsewhere (a triage, a fetch, papers found, a subscription): the list again,
      // which nothing else asks for after a change
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

  /** The tab opened: the subscriptions, the saved views, and a fetch when the last is older than 15 minutes. */
  async function start() {
    try {
      [last, subscriptions, views] = await Promise.all([api.lastFetch(), api.subscriptions(), api.savedViews()]);
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

  /** ⇧⌘R, ↻: every subscription fetched, or the one of id `only` (the phases of the fetch say where it is; one
   *  asked while another reads the feeds waits for it, and a full one then answers its summary); what it brought
   *  shown at once, unless `show` is off (the fetch of a tab opening). */
  async function fetchNow(show = true, only?: string) {
    failures = false;
    try {
      const summary = await api.fetchFeeds(only);
      const failed = summary.failed.length;
      if (failed) list.say(`${failed} ${failed === 1 ? "subscription" : "subscriptions"} failed`);
    } catch (e) {
      list.say(String(e));
    }
    if (show) bringUpToDate(true);
  }

  /** The list brought up to date: what was fetched since it was shown, held back, into it. ⌘R (`keep` off) also
   *  lets go the items triaged here, which leave where they no longer belong, and sorts it again; *N new · Show*
   *  and a fetch asked for here keep them in place. */
  function bringUpToDate(keep: boolean) {
    if (!keep) list.sticky.clear();
    shown = new Date().toISOString();
    load(keep);
  }

  /** A fetch read the feeds, or ended: its summary and the subscriptions again. */
  async function readFetch() {
    try {
      [last, subscriptions] = await Promise.all([api.lastFetch(), api.subscriptions()]);
    } catch {
      // the list says what is wrong
    }
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

  /** The items of some ids that the actions know: the details shown (they say its link), else the rows. */
  const itemsOf = (ids: string[]): Item[] =>
    ids.flatMap((id): Item[] => {
      const item = list.detail?.id === id ? list.detail : list.byId.get(id);
      return item ? [item] : [];
    });

  /** An action as an item of a menu. */
  const menuItem = (action: Action, ids: string[], items: Item[]) =>
    MenuItem.new({ text: titled(action.label(items)), enabled: action.enabled(items), action: () => action.run(ids) });

  /** s, x, m: the triage of the items selected toggled (`toggled`); an item not in the list (a linked paper) is
   *  asked for. */
  async function triage(action: TriageAction, ids: string[]) {
    const found = await Promise.all(ids.map((id) => list.byId.get(id) ?? list.detailOf(id)));
    const items = found.filter((r): r is FeedRow | FeedDetail => !!r);
    const change = toggled(items, action);
    if (change?.triage === "seen") markRead(change.ids);
    else if (change) list.setTo(change.ids, change.triage);
  }

  /** ⇧Space, the menu: the new items above the one in focus (the menu's row) marked read. */
  function markAbove(from = list.selection.focus) {
    markRead(newAbove(list.rows, from));
  }

  /** Mark read ▾: the new items of the list, those older than some days, those above the selection, marked seen
   *  (above 50, after asking). */
  async function markMenu() {
    const item = (text: string, ids: string[]) =>
      MenuItem.new({ text: `${text} (${ids.length})`, enabled: ids.length > 0, action: () => markRead(ids) });
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

  /** Items marked read (above 50, after asking), said. */
  async function markRead(ids: string[]) {
    if (!ids.length) return;
    const question = `Mark ${ids.length} items as read?`;
    if (ids.length > MARK_ASK && !(await ask(question, { title: "Mark read", okLabel: "Mark read", cancelLabel: "Cancel" }))) return;
    list.setTo(ids, "seen");
    list.say(`Marked ${ids.length} read`);
  }

  /** U: kept unread (a reminder due now, first in the list); the reminders of items all kept so, cleared. */
  function keepUnread(ids: string[]) {
    const due = ids.every((id) => list.byId.get(id)?.reminder === "due");
    if (due) return list.setTo(ids, "new");
    remindOf(ids, null);
  }

  /** Items new again, to be reminded of in some days (snoozed till then), else now (kept unread); shown at once,
   *  the list again if refused. */
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
      load(true);
    }
  }

  /** Z, Later ▾: kept unread, snoozed for some days, or the reminder cleared. */
  async function laterMenu(ids: string[]) {
    if (!ids.length) return;
    const items = itemsOf(ids);
    const menu = await Menu.new({
      items: [
        await menuItem(actions.keepUnread, ids, items),
        await PredefinedMenuItem.new({ item: "Separator" }),
        ...(await Promise.all(SNOOZES.map((s) => MenuItem.new({ text: titled(`Snooze ${s.text}`), action: () => remindOf(ids, s.days) })))),
        await PredefinedMenuItem.new({ item: "Separator" }),
        await menuItem(actions.clearReminder, ids, items),
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
    const items = itemsOf(ids);
    if (!items.length) return;
    const item = (name: ActionName) => menuItem(actions[name], ids, items);
    const separator = () => PredefinedMenuItem.new({ item: "Separator" });
    // one item: the kind of each subscription that lists it, to set right
    const sources = items.length === 1 ? ((await list.detailOf(id))?.sources ?? []) : [];
    const kindOf = async (s: api.FeedDetail["sources"][number]) =>
      Submenu.new({
        text: sources.length === 1 ? "Source Is" : `${s.title} Is`,
        items: await Promise.all(
          sourceKinds.map((kind) =>
            CheckMenuItem.new({ text: titled(SOURCE_KINDS[kind].name), checked: s.kind === kind, action: () => setKind(s.id, s.title, kind) }),
          ),
        ),
      });
    const menu = await Menu.new({
      items: [
        await item("open"),
        await item("add"),
        await item("addBare"),
        ...(actions.find.enabled(items) ? [await item("find")] : []),
        await separator(),
        await item("star"),
        await item("read"),
        await item("dismiss"),
        await item("keepUnread"),
        await Submenu.new({
          text: "Snooze",
          items: await Promise.all(SNOOZES.map((s) => MenuItem.new({ text: titled(s.text), action: () => remindOf(ids, s.days) }))),
        }),
        await item("clearReminder"),
        await separator(),
        await MenuItem.new({ text: "Mark All Above as Read", action: () => markAbove(id), enabled: newAbove(list.rows, id).length > 0 }),
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
    const feeds = subs.filter((s) => "url" in s.feed);
    const menu = await Menu.new({
      items: [
        ...(feeds.length
          ? [
              await Submenu.new({
                text: "Source Is",
                items: await Promise.all(
                  sourceKinds.map((kind) =>
                    CheckMenuItem.new({
                      text: titled(SOURCE_KINDS[kind].name),
                      checked: feeds.every((s) => s.kind === kind),
                      action: async () => {
                        for (const s of feeds) await setKind(s.id, s.title, kind);
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
  }

  /** A subscription ended, here or in the Subscribe sheet. */
  function unsubscribed(id: string) {
    const gone = subscriptions.find((s) => s.id === id);
    subscriptions = subscriptions.filter((s) => s.id !== id);
    if (gone && !subscriptions.some((s) => s.title === gone.title)) {
      filters.sources = filters.sources.filter((t) => t !== gone.title);
    }
  }

  /** A subscription set to another kind, from the menus: its items move at once, it to the sources of that kind. */
  async function setKind(id: string, title: string, kind: api.SourceKind) {
    try {
      const info = await api.updateSubscription(id, { kind });
      subscriptions = subscriptions.map((s) => (s.id === id ? info : s));
      list.say(`${title}: ${SOURCE_KINDS[kind].name}`);
    } catch (e) {
      list.say(String(e));
    }
  }

  /** The Subscribe sheet closed: what was subscribed to is fetched. */
  function closeSubscribe() {
    subscribing = false;
    if (subscribed) fetchNow();
    subscribed = false;
  }

  /** ⌘↩: the Review of the starred items; again, or Esc in it, the list. */
  function toggleReview() {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    reviewing = !reviewing;
  }


  /** ⌘F, /: the search field. */
  export function focusSearch() {
    list.focusSearch();
  }

  /** A key of the window while the tab is in front (none while a sheet is open): ⌘R brings the list up to date,
   *  ⇧⌘R fetches, ⌘↩ starts the Review (which then has the keys); the list's keys move and read (`navKey`); the
   *  keys of the actions act on the selection (s, x, m, u, z, o, a, f); ⇧Space marks all above as read. */
  export function key(e: KeyboardEvent) {
    if (modalOpen()) return;
    const act = (action: () => unknown) => {
      e.preventDefault();
      action();
    };
    if (matches(e, REFRESH)) return act(() => bringUpToDate(false));
    if (matches(e, FETCH)) return act(() => fetchNow());
    if (matches(e, REVIEW)) return act(toggleReview);
    if (reviewing && !typing(e)) return reviewer?.key(e);
    if (list.navKey(e) || typing(e) || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === " " && e.shiftKey) return act(() => markAbove());
    if (e.shiftKey) return;
    const action = Object.values(actions).find((a) => a.key === e.key);
    if (action) act(() => action.run(list.targets()));
  }

  /** The saved views changed: kept (their counts come with the list again). */
  async function keepViews(saved: api.SavedView[]) {
    try {
      await api.setSavedViews(saved);
      views = saved;
    } catch (e) {
      list.say(String(e));
    }
  }

  /** A saved view shown: its choices, search and order in place of the tab's. */
  function applyView(name: string) {
    const view = views.find((v) => sameName(v.name, name));
    if (view) filters = applied(filters, view);
  }

  /** What the tab shows, as a saved view. */
  const current = (name: string): api.SavedView => ({ name, query: queryOf(filters), sort: filters.sort });

  /** Why a view cannot be named so (that of `own`, being renamed, aside); none: it can. */
  const viewRefusal = (name: string, own?: string) => refusal(views, name, own, "view");

  function dropChip(chip: FeedChip) {
    filters = withoutChip(filters, chip);
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
  {#if tab.showSidebar}
    <aside class="sidebar" style:width="{tab.sidebar}px">
      <FeedsSidebar
        bind:filters
        {counts}
        colours={tab.colours}
        {groups}
        {keepDays}
        onsourcemenu={sourceMenu}
        onsubscribe={() => (subscribing = true)}
      >
        {#snippet saved()}
          <SavedSearches
            searches={views.map((v) => ({ name: v.name, count: counts?.views?.[v.name] ?? 0 }))}
            heading="Saved views"
            noun="view"
            shown={shownView}
            refusal={viewRefusal}
            onapply={applyView}
            onrename={(name, to) => keepViews(renamed(views, name, to))}
            onupdate={(name) => keepViews(changed(views, name, () => current(name)))}
            ondelete={(name) => keepViews(removed(views, name))}
          />
        {/snippet}
      </FeedsSidebar>
    </aside>
    <Splitter bind:width={tab.sidebar} side="left" min={160} max={420} />
  {/if}
  <div class="right">
    <header>
      {@render tab.tabs()}
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
        shown={shownView !== null}
        noun="view"
        suggestion={chips.map((c) => c.text).join(" · ") || "All items"}
        refusal={viewRefusal}
        onsave={(name) => keepViews(added(views, current(name)))}
      />
      <span class="notice">{list.notice ?? ""}</span>
      <span class="spacer"></span>
      {#if held}
        <button class="tool held" title="Show what was fetched since the list was brought up to date ({label(REFRESH)} also sorts it again)" onclick={() => bringUpToDate(true)}>
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
        {reviewing ? "Leave review" : `Review starred (${starred})`}
      </button>
    </header>
    {#if reviewing}
      <Review
        bind:this={reviewer}
        colours={tab.colours}
        onadd={list.add}
        ontriage={(id, t) => list.setTo([id], t)}
        onopen={(id) => list.openItems([id])}
        onitem={(id) => {
          toggleReview();
          list.showItem(id);
        }}
        onentry={tab.onentry}
        onclose={toggleReview}
      />
    {:else}
      <ItemPane
        {list}
        table={{
          columns,
          specs: FEED_SPECS,
          menu: FEED_ORDER,
          sort: filters.sort,
          onsort: (sort) => (filters.sort = sort),
          oncolumns: (shown) => (columns = shown),
          onstar: (id) => actions.star.run([id]),
          onmenu: rowMenu,
        }}
        none="No item selected"
        bar={listBar}
        empty={loaded && list.rows.length ? undefined : noRows}
        actions={itemActions}
        finding={finding !== null && finding === list.detail?.id}
        onaddlink={(link: Linked) => list.add({ id: link.doi, item: link.item }, true)}
        onstarlink={(item) => actions.star.run([item])}
        onfind={find}
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
        {subscriptions.length} {subscriptions.length === 1 ? "subscription" : "subscriptions"} · Starred {starred} · Added
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
    {@const ids = list.chosen()}
    {@const items = itemsOf(ids)}
    <div class="selbar">
      <span>{list.selection.selected.size} selected</span>
      {#each BAR_BUTTONS as name (name)}
        {@const action = actions[name]}
        <button disabled={!action.enabled(items)} onclick={() => action.run(ids)}>{buttonText(name, action.label(items))}</button>
      {/each}
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

<!-- the buttons of the details, a post's or a paper's: on the selection when the item is in it -->
{#snippet itemActions(d: FeedDetail)}
  {@const ids = list.targetsOr(d.id)}
  {@const items = itemsOf(ids)}
  {#each isPost(d.kind) ? POST_BUTTONS : PAPER_BUTTONS as name (name)}
    {@const action = actions[name]}
    <button class:primary={name === "add"} class:open={name === "open"} title={action.hint} disabled={!action.enabled(items)} onclick={() => action.run(ids)}>
      {buttonText(name, action.label(items))}{#if action.key}{" "}<kbd>{action.key.toUpperCase()}</kbd>{/if}
    </button>
  {/each}
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
