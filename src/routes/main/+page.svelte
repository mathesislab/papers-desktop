<script lang="ts">
  // The main window: its tabs, Library (LibraryView, with the papers open), Feeds (FeedsView) and Search
  // (SearchView), the last two unless hidden by their settings; its layout, kept between sessions (a tab's search
  // cleared when it comes back after a while unused, the setting `clear_after_minutes`); back and forward
  // through the places it showed (navigation.ts); the commands of the menu and the keys, the window's own or passed
  // on to the tab in front.

  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, tick, untrack } from "svelte";
  import "$lib/theme.css";
  import { commands } from "$lib/commands";
  import { settings as feedSettings } from "$lib/feeds/api";
  import { interestColours } from "$lib/feeds/filters";
  import FeedsView from "$lib/feeds/FeedsView.svelte";
  import { label, mac, matches, zoomStep } from "$lib/keys";
  import { USE_STEP, cleared, restore, stale, type Layout, type Tab } from "$lib/layout";
  import LibraryView from "$lib/library/LibraryView.svelte";
  import { setTabContext } from "$lib/list/tab";
  import { NO_HISTORY, goBack, goForward, kept, placeOf, placeText, went, type History, type Place } from "$lib/navigation";
  import SearchView from "$lib/search/SearchView.svelte";
  import { onToggle, setToggle as saveToggle, toggles, type Toggle } from "$lib/toggles";
  import { CheckMenuItem, Menu } from "@tauri-apps/api/menu";
  import { WINDOW_COLOURS, cssOf, onWindowColour, setWindowColour, windowColour, type WindowColour } from "$lib/windowColour";

  /** the tabs of the window, in their order: the switch that shows each, but the library's, always there */
  const TABS: { tab: Tab; name: string; title: string; toggle: Toggle | null }[] = [
    { tab: "library", name: "Library", title: "Library", toggle: null },
    { tab: "feeds", name: "Feeds", title: "Feeds", toggle: "feeds-tab" },
    { tab: "search", name: "Search", title: "Search OpenAlex's works", toggle: "search-tab" },
  ];

  let layout = $state<Layout>(restore(null));
  /** the tabs shown: their switches (Settings → Feeds, Window → Show Feeds Tab, Show Search Tab) */
  let shownTabs = $state<Record<Tab, boolean>>({ library: true, feeds: true, search: true });
  /** the details of the Feeds and Search tabs beside the list: the switch `feeds-beside` */
  let beside = $state(false);
  /** the details of the Library tab beside the list: the switch `library-beside` */
  let libraryBeside = $state(true);
  /** the colours of the interests of the feeds' settings, for both tabs */
  let colours = $state(new Map<string, string>());
  let loaded = false;
  let timer: ReturnType<typeof setTimeout>;

  /** back and forward: the places shown before and after the one shown */
  let history = $state<History>(NO_HISTORY);
  /** the place shown, as the window shows it */
  let place = $derived(placeOf(layout.tab, layout.active));
  /** the place last shown: when the place shown changes, it goes back (`went`); back and forward set it first */
  let lastPlace: Place | null = null;

  let library = $state<ReturnType<typeof LibraryView>>();
  let feeds = $state<ReturnType<typeof FeedsView>>();
  let search = $state<ReturnType<typeof SearchView>>();

  setTabContext({
    tabs,
    get beside() {
      return beside;
    },
    get sidebar() {
      return layout.sidebar;
    },
    set sidebar(width) {
      layout.sidebar = width;
    },
    get details() {
      return layout.details;
    },
    set details(width) {
      layout.details = width;
    },
    get showSidebar() {
      return layout.showSidebar;
    },
    get showDetails() {
      return layout.showDetails;
    },
    set showDetails(on) {
      layout.showDetails = on;
    },
    get colours() {
      return colours;
    },
    // the Add sheet is the library's, over the tab: after the paper is added, the tab stays in front
    onadd: (initial) => library?.addFrom(initial) ?? Promise.resolve(null),
    onentry: (id) => library?.showEntry(id),
    onpdf: (id) => library?.openEntry(id),
    libraryOf: (id) => library?.libraryOf(id) ?? null,
  });

  /** the colour of the bar at the top, to tell the window apart (the setting) */
  let colour = $state<WindowColour>("none");

  /** A right-click on the bar: its colour to choose. */
  async function colourMenu(e: MouseEvent) {
    e.preventDefault();
    const items = WINDOW_COLOURS.map((c) =>
      CheckMenuItem.new({ text: c.name, checked: colour === c.colour, action: () => setWindowColour(c.colour) }),
    );
    await (await Menu.new({ items: await Promise.all(items) })).popup();
  }

  /** the minutes after which a tab not used has its search cleared (the setting); 0: never */
  let clearAfter = 60;

  /** The window is made hidden, not to flash empty: shown once drawn. */
  onMount(() => {
    // from the menu of the app
    const unlisten = listen<string>("command", (e) => command(e.payload));
    const unlistenToggle = onToggle(setToggle);
    // back from the settings: the interests again; back after a while: the tab in front without its search
    const unlistenFocus = getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (!focused) return;
      readColours();
      used();
    });
    const unlistenSettings = listen("settings", readClearAfter);
    const unlistenOnline = listen<string>("search-online", (e) => searchOnline(e.payload));
    const unlistenColour = onWindowColour((c) => (colour = c));
    windowColour().then((c) => (colour = c));
    load();
    readColours();
    readClearAfter();
    return () => {
      unlisten.then((f) => f());
      unlistenToggle();
      unlistenFocus.then((f) => f());
      unlistenSettings.then((f) => f());
      unlistenOnline.then((f) => f());
      unlistenColour();
    };
  });

  async function readClearAfter() {
    const { settings } = await invoke<{ settings: { clear_after_minutes: number } }>("get_settings");
    clearAfter = settings.clear_after_minutes;
  }

  /** The tab in front used now (a key, a click, the window back, the tab shown): first its search cleared if it
   *  was not used for longer than the setting; noted, at most once a minute (the layout is saved with it). */
  function used(tab: Tab = layout.tab) {
    if (!loaded) return;
    const now = Date.now();
    if (stale(layout.used, tab, now, clearAfter)) clear(tab);
    if (now - (layout.used[tab] ?? 0) > USE_STEP) layout.used = { ...layout.used, [tab]: now };
  }

  /** The search and filters of a tab as at first (`cleared`); the Search tab in front clears its list too. */
  function clear(tab: Tab) {
    if (tab === "search" && layout.tab === "search" && search) return search.clearSearch();
    const next = cleared(layout, tab);
    if (tab === "library") layout.library = next.library;
    else if (tab === "feeds") layout.feeds.filters = next.feeds.filters;
    else layout.search = next.search;
  }

  async function load() {
    const [saved, on] = await Promise.all([invoke<Partial<Layout> | null>("layout"), toggles()]);
    layout = restore(saved);
    for (const [name, value] of Object.entries(on)) setToggle(name as Toggle, value);
    loaded = true;
    used();
    await library?.load();
    await tick();
    const window = getCurrentWindow();
    await window.show();
    await window.setFocus();
    // the window made for the library hotkey ("main?library"), or for a search of OpenAlex ("main?online=QUERY")
    const asked = new URLSearchParams(location.search);
    const online = asked.get("online");
    if (asked.has("library")) searchLibrary();
    else if (online !== null) searchOnline(online);
    else if (layout.tab === "library") library?.focusSearch();
  }

  /** The panel's Search OpenAlex: the Search tab (shown if it was hidden), searching for `query`. */
  async function searchOnline(query: string) {
    if (!shownTabs.search) {
      setToggle("search-tab", true);
      saveToggle("search-tab", true).catch(() => {});
    }
    showTab("search");
    await tick();
    search?.ask(query);
  }

  /** The library hotkey: the Library tab, the list in front of the papers, its search field ready (its text
   *  selected). */
  async function searchLibrary() {
    showTab("library");
    library?.command("library");
    await tick();
    library?.command("find");
  }

  /** The colours of the interests, as the settings name them. */
  async function readColours() {
    try {
      colours = interestColours((await feedSettings()).interests.map((i) => i.name));
    } catch {
      // the interests show without their colours
    }
  }

  /** A tab of the window to the front. The focus does not stay in the one that goes; the library's search field
   *  takes it, unless a paper is in front (its PDF has the keys). */
  function showTab(tab: Tab) {
    if (layout.tab === tab) return;
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    // the tab left was used till now; the tab shown, if long unused, without its search
    layout.used = { ...layout.used, [layout.tab]: Date.now() };
    used(tab);
    layout.tab = tab;
    if (tab === "library") tick().then(() => library?.focusSearch());
  }

  // each place shown, once the layout is read, a step
  $effect(() => {
    const now = place;
    untrack(() => {
      if (lastPlace && loaded) history = went(history, lastPlace, now);
      lastPlace = now;
    });
  });

  // a paper closed: its places go
  $effect(() => {
    const open = layout.open;
    untrack(() => (history = kept(history, open, place)));
  });

  /** Back or forward (`goBack`, `goForward`): the place before or after shown, unless the BibTeX being edited stays. */
  async function go(step: typeof goBack) {
    const from = place;
    const next = step(history, from);
    if (!next) return;
    lastPlace = next.to;
    if (next.to.tab === "library" && library && !(await library.showPlace(next.to.paper))) {
      lastPlace = from;
      return;
    }
    history = next.history;
    showTab(next.to.tab);
  }

  /** What back or forward goes to, on hover. */
  const stepTitle = (word: string, to: Place | undefined, keys: string) =>
    to ? `${word} to ${placeText(to, (id) => library?.titleOf(id))} (${keys})` : word;

  /** A switch set: a tab shown or hidden (hidden in front, the Library tab comes), the details beside or not. */
  function setToggle(toggle: Toggle, on: boolean) {
    if (toggle === "feeds-beside") {
      beside = on;
      return;
    }
    if (toggle === "library-beside") {
      libraryBeside = on;
      return;
    }
    const tab = TABS.find((t) => t.toggle === toggle)!.tab;
    shownTabs[tab] = on;
    if (!on && layout.tab === tab) showTab("library");
  }

  // saved a moment after a change: a pane dragged changes it many times
  $effect(() => {
    const snapshot = $state.snapshot(layout);
    if (!loaded) return;
    clearTimeout(timer);
    timer = setTimeout(() => invoke("save_layout", { layout: snapshot }), 300);
  });

  /** A command of the window, from the menu of the app or its keys: the window's own, else the tab's in front. */
  function command(name: string) {
    switch (name) {
      // from the Feeds tab, the Library tab as it was left; in it, the library in front of the papers
      case "library":
        return layout.tab === "library" ? library?.command(name) : showTab("library");
      case "feeds":
      case "search":
        return shownTabs[name] ? showTab(name) : undefined;
      case "search-library":
        return searchLibrary();
      // the PDF in front has the keys; else they set the text size
      case "zoom-in":
        return zoom(1);
      case "zoom-out":
        return zoom(-1);
      case "zoom-fit":
        return zoom(0);
      case "sidebar":
        layout.showSidebar = !layout.showSidebar;
        return;
      case "details":
        layout.showDetails = !layout.showDetails;
        return;
      case "settings":
        invoke("open_settings");
        return;
      case "back":
        return go(goBack);
      case "forward":
        return go(goForward);
      case "undo":
        return undo(true);
      case "redo":
        return undo(false);
      default:
        return layout.tab === "library" ? library?.command(name) : tabCommand(name);
    }
  }

  /** A command in the Feeds or the Search tab: ⌘W closes the window, ⌘N opens the Add sheet of the library (and
   *  Export… exports from it), ⌘F
   *  goes to the tab's search field; the papers open and their keys are the Library tab's. */
  function tabCommand(name: string) {
    switch (name) {
      case "close":
        return library?.closeWindow();
      case "add":
      case "export":
        return library?.command(name);
      case "find":
        return (layout.tab === "feeds" ? feeds : search)?.focusSearch();
    }
  }

  function zoom(steps: number) {
    if (layout.tab !== "library" || !library?.zoom(steps)) invoke("text_size_step", { steps });
  }

  /** ⌘Z, ⌘⇧Z: in a field, the field's own undo; elsewhere the library's, in the Library tab. */
  function undo(back: boolean) {
    const el = document.activeElement;
    if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || (el instanceof HTMLElement && el.isContentEditable)) {
      document.execCommand(back ? "undo" : "redo");
      return;
    }
    if (layout.tab === "library") library?.stepLibrary(back);
  }

  function onkeydown(e: KeyboardEvent) {
    used();
    // the question before a delete has the keys
    if (library?.asking()) return;
    // ⌃Tab: the menu of macOS does not get it from a field of the page
    if (e.ctrlKey && e.key === "Tab") {
      e.preventDefault();
      command(e.shiftKey ? "previous-tab" : "next-tab");
      return;
    }
    // ⌘+ also on a keyboard where it is ⌘⇧=; on macOS ⌘+ is of the menu (which shows it), and ⌘= comes here
    const step = zoomStep(e);
    if ((!mac || (e.key === "=" && !e.shiftKey)) && step !== null) {
      e.preventDefault();
      command(step > 0 ? "zoom-in" : step < 0 ? "zoom-out" : "zoom-fit");
      return;
    }
    // ⌘⇧] and ⌘⇧[: from the menu on macOS; here when the menu does not take them
    for (const name of ["show-next-tab", "show-previous-tab"]) {
      if (mac && matches(e, commands[name])) {
        e.preventDefault();
        command(name);
        return;
      }
    }
    // the commands: from the menu on macOS
    if (!mac) {
      const name = Object.keys(commands).find((c) => matches(e, commands[c]));
      if (name) {
        e.preventDefault();
        command(name);
        return;
      }
    }
    // the keys of the tab in front only: the other's stay off
    if (layout.tab === "library") library?.key(e);
    else if (layout.tab === "feeds") feeds?.key(e);
    else search?.key(e);
  }
</script>

<svelte:window {onkeydown} onpointerdown={() => used()} />

<!-- the window's colour, at its top, to tell it apart among others; a right-click chooses it -->
{#if cssOf(colour)}
  <div class="colour" style:background={cssOf(colour)} title="The colour of the window: right-click to choose" oncontextmenu={colourMenu} role="presentation"></div>
{/if}

<!-- back and forward; no switch with the Feeds and Search tabs hidden: the Library tab alone -->
{#snippet tabs()}
  {@const shown = TABS.filter((t) => shownTabs[t.tab])}
  <div class="steps">
    <button
      disabled={!history.back.length}
      aria-label="Back"
      title={stepTitle("Back", history.back.at(-1), label(commands.back))}
      onclick={() => go(goBack)}>‹</button
    >
    <button
      disabled={!history.forward.length}
      aria-label="Forward"
      title={stepTitle("Forward", history.forward[0], label(commands.forward))}
      onclick={() => go(goForward)}>›</button
    >
  </div>
  {#if shown.length > 1}
    <div class="tabs" role="tablist">
      {#each shown as t (t.tab)}
        <button
          role="tab"
          aria-selected={layout.tab === t.tab}
          class={t.tab}
          class:on={layout.tab === t.tab}
          title="{t.title} ({label(commands[t.tab])})"
          onclick={() => showTab(t.tab)}>{t.name}</button
        >
      {/each}
    </div>
  {/if}
{/snippet}

<!-- the library stays, hidden behind the feeds: its selection, scroll and edit are there when it comes back -->
<LibraryView
  bind:this={library}
  bind:layout
  visible={layout.tab === "library"}
  beside={libraryBeside}
  {tabs}
  onfront={() => showTab("library")}
/>
{#if shownTabs.feeds && layout.tab === "feeds"}
  <FeedsView bind:this={feeds} bind:filters={layout.feeds.filters} bind:columns={layout.feeds.columns} bind:look={layout.feeds.look} bind:shown={layout.feeds.shown} />
{:else if shownTabs.search && layout.tab === "search"}
  <SearchView bind:this={search} bind:search={layout.search} />
{/if}

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    background: var(--bg);
    color: var(--fg);
    font-size: 13px;
    cursor: default;
    -webkit-user-select: none;
    user-select: none;
  }

  /* Library | Feeds | Search, at the start of the toolbar of each tab */
  /* absolute, not fixed: WebKit tints the title bar with what is fixed at the top of the page (the window does not
     scroll: it stays there) */
  .colour {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 50;
    height: 6px;
  }
  .steps {
    flex: none;
    display: flex;
    gap: 2px;
  }
  .steps button {
    width: 22px;
    padding: 0 0 2px;
    border: none;
    border-radius: 5px;
    background: none;
    color: var(--muted);
    font: inherit;
    font-size: 18px;
    line-height: 22px;
  }
  .steps button:hover:not(:disabled) {
    background: var(--line);
    color: var(--fg);
  }
  .steps button:disabled {
    opacity: 0.35;
  }
  .tabs {
    flex: none;
    display: flex;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  /* each tab in its colour: a light tint when behind, so that each is told apart at a glance; full in front */
  .tabs button {
    --c: var(--accent);
    --on-c: var(--on-accent);
    /* a width of its own: the bold of the tab in front does not move the other */
    width: 64px;
    padding: 3px 0;
    border: none;
    background: color-mix(in srgb, var(--c) 11%, transparent);
    color: var(--c);
    font: inherit;
    font-size: 12px;
  }
  .tabs button.feeds {
    --c: var(--feeds);
    --on-c: var(--on-feeds);
  }
  .tabs button.search {
    --c: var(--search);
    --on-c: var(--on-search);
  }
  .tabs button + button {
    border-left: 1px solid var(--border);
  }
  .tabs button:hover:not(.on) {
    background: color-mix(in srgb, var(--c) 20%, transparent);
  }
  .tabs button.on {
    background: var(--c);
    color: var(--on-c);
    font-weight: 600;
  }
</style>
