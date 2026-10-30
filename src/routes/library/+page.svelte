<script lang="ts">
  // The main window: its layout, kept between sessions; the commands of the menu and the keys, the window's own or
  // passed on to the library (LibraryView).

  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, tick } from "svelte";
  import "$lib/theme.css";
  import { commands } from "$lib/commands";
  import { mac, matches, zoomStep } from "$lib/keys";
  import { restore, type Layout } from "$lib/layout";
  import LibraryView from "$lib/library/LibraryView.svelte";

  let layout = $state<Layout>(restore(null));
  let loaded = false;
  let timer: ReturnType<typeof setTimeout>;

  let library = $state<ReturnType<typeof LibraryView>>();

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
    library?.focusSearch();
  }

  // saved a moment after a change: a pane dragged changes it many times
  $effect(() => {
    const snapshot = $state.snapshot(layout);
    if (!loaded) return;
    clearTimeout(timer);
    timer = setTimeout(() => invoke("save_layout", { layout: snapshot }), 300);
  });

  /** A command of the window, from the menu of the app or its keys: the window's own, else the library's. */
  function command(name: string) {
    switch (name) {
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
        return library?.command(name);
    }
  }

  function zoom(steps: number) {
    if (!library?.zoom(steps)) invoke("text_size_step", { steps });
  }

  /** ⌘Z, ⌘⇧Z: in a field, the field's own undo; elsewhere the library's. */
  function undo(back: boolean) {
    const el = document.activeElement;
    if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || (el instanceof HTMLElement && el.isContentEditable)) {
      document.execCommand(back ? "undo" : "redo");
      return;
    }
    library?.stepLibrary(back);
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
    library?.key(e);
  }
</script>

<svelte:window {onkeydown} />

<LibraryView bind:this={library} bind:layout />

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
</style>
