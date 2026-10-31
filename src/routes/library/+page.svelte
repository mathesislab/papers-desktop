<script lang="ts">
  // The main window: its two tabs, Library (LibraryView, with the papers open) and Feeds (FeedsView); its layout,
  // kept between sessions; the commands of the menu and the keys, the window's own or passed on to the tab in front.

  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, tick } from "svelte";
  import "$lib/theme.css";
  import { commands } from "$lib/commands";
  import FeedsView from "$lib/feeds/FeedsView.svelte";
  import { label, mac, matches, zoomStep } from "$lib/keys";
  import { restore, type Layout } from "$lib/layout";
  import LibraryView from "$lib/library/LibraryView.svelte";

  let layout = $state<Layout>(restore(null));
  let loaded = false;
  let timer: ReturnType<typeof setTimeout>;

  let library = $state<ReturnType<typeof LibraryView>>();
  let feeds = $state<ReturnType<typeof FeedsView>>();

  /** The window is made hidden, not to flash empty: shown once drawn. */
  onMount(() => {
    // from the menu of the app
    const unlisten = listen<string>("command", (e) => command(e.payload));
    load();
    return () => unlisten.then((f) => f());
  });

  async function load() {
    layout = restore(await invoke<Partial<Layout> | null>("layout"));
    loaded = true;
    await library?.load();
    await tick();
    const window = getCurrentWindow();
    await window.show();
    await window.setFocus();
    if (layout.view === "library") library?.focusSearch();
  }

  /** A tab of the window to the front. The focus does not stay in the one that goes; the library's search field
   *  takes it, unless a paper is in front (its PDF has the keys). */
  function showView(view: Layout["view"]) {
    if (layout.view === view) return;
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    layout.view = view;
    if (view === "library") tick().then(() => library?.focusSearch());
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
        return layout.view === "library" ? library?.command(name) : showView("library");
      case "feeds":
        return showView("feeds");
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
      case "undo":
        return undo(true);
      case "redo":
        return undo(false);
      default:
        return layout.view === "library" ? library?.command(name) : feedsCommand(name);
    }
  }

  /** A command in the Feeds tab: ⌘W closes the window, ⌘N opens the Add sheet of the library, ⌘F goes to the
   *  search of the feeds; the papers open and their keys are the Library tab's. */
  function feedsCommand(name: string) {
    switch (name) {
      case "close":
        return library?.closeWindow();
      case "add":
        return library?.command(name);
      case "find":
        return feeds?.focusSearch();
    }
  }

  function zoom(steps: number) {
    if (layout.view !== "library" || !library?.zoom(steps)) invoke("text_size_step", { steps });
  }

  /** ⌘Z, ⌘⇧Z: in a field, the field's own undo; elsewhere the library's, in the Library tab. */
  function undo(back: boolean) {
    const el = document.activeElement;
    if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || (el instanceof HTMLElement && el.isContentEditable)) {
      document.execCommand(back ? "undo" : "redo");
      return;
    }
    if (layout.view === "library") library?.stepLibrary(back);
  }

  function onkeydown(e: KeyboardEvent) {
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
    if (layout.view === "library") library?.key(e);
    else feeds?.key(e);
  }
</script>

<svelte:window {onkeydown} />

{#snippet tabs()}
  <div class="tabs" role="tablist">
    <button
      role="tab"
      aria-selected={layout.view === "library"}
      class:on={layout.view === "library"}
      title="Library ({label(commands.library)})"
      onclick={() => showView("library")}>Library</button
    >
    <button
      role="tab"
      aria-selected={layout.view === "feeds"}
      class:on={layout.view === "feeds"}
      title="Feeds ({label(commands.feeds)})"
      onclick={() => showView("feeds")}>Feeds</button
    >
  </div>
{/snippet}

<!-- the library stays, hidden behind the feeds: its selection, scroll and edit are there when it comes back -->
<LibraryView
  bind:this={library}
  bind:layout
  visible={layout.view === "library"}
  {tabs}
  onfront={() => showView("library")}
/>
{#if layout.view === "feeds"}
  <!-- the Add sheet is the library's, over this tab: after the paper is added, the Feeds tab stays in front -->
  <FeedsView
    bind:this={feeds}
    {tabs}
    bind:filters={layout.feeds}
    bind:sidebar={layout.sidebar}
    bind:details={layout.details}
    showSidebar={layout.showSidebar}
    showDetails={layout.showDetails}
    onadd={(initial) => library?.addFrom(initial) ?? Promise.resolve(null)}
    onentry={(id) => library?.showEntry(id)}
  />
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

  /* Library | Feeds, at the start of the toolbar of either tab */
  .tabs {
    flex: none;
    display: flex;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .tabs button {
    /* a width of its own: the bold of the tab in front does not move the other */
    width: 64px;
    padding: 3px 0;
    border: none;
    background: none;
    color: var(--muted);
    font: inherit;
    font-size: 12px;
  }
  .tabs button + button {
    border-left: 1px solid var(--border);
  }
  .tabs button.on {
    background: var(--line);
    color: var(--fg);
    font-weight: 600;
  }
</style>
