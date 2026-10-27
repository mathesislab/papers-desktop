<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, tick } from "svelte";
  import "$lib/theme.css";
  import { label, matches, type Shortcut } from "$lib/keys";
  import Splitter from "$lib/library/Splitter.svelte";

  /** What the window keeps between sessions (window.json, in the data folder
   *  of the app): the widths of the side panes, whether the details show. */
  type Layout = { sidebar: number; details: number; showDetails: boolean };
  const DEFAULTS: Layout = { sidebar: 230, details: 320, showDetails: true };

  let layout = $state<Layout>({ ...DEFAULTS });
  let loaded = false;
  let timer: ReturnType<typeof setTimeout>;

  const detailsShortcut: Shortcut = { key: "i", mod: true, alt: true };
  const settingsShortcut: Shortcut = { key: ",", mod: true };

  /** The window is made hidden, not to flash empty: shown once drawn. */
  onMount(async () => {
    const saved = await invoke<Partial<Layout> | null>("layout");
    layout = { ...DEFAULTS, ...saved };
    loaded = true;
    await tick();
    const window = getCurrentWindow();
    await window.show();
    await window.setFocus();
  });

  // saved a moment after a change: a pane dragged changes it many times
  $effect(() => {
    const snapshot = $state.snapshot(layout);
    if (!loaded) return;
    clearTimeout(timer);
    timer = setTimeout(() => invoke("save_layout", { layout: snapshot }), 300);
  });

  function onkeydown(e: KeyboardEvent) {
    if (matches(e, detailsShortcut)) {
      e.preventDefault();
      layout.showDetails = !layout.showDetails;
    } else if (matches(e, settingsShortcut)) {
      e.preventDefault();
      invoke("open_settings");
    }
  }
</script>

<svelte:window {onkeydown} />

<main>
  <aside class="sidebar" style:width="{layout.sidebar}px"></aside>
  <Splitter bind:width={layout.sidebar} side="left" min={160} max={420} />
  <section class="library">
    <header>
      <span class="title">Library</span>
      <button
        class:on={layout.showDetails}
        title="Details ({label(detailsShortcut)})"
        onclick={() => (layout.showDetails = !layout.showDetails)}>Details</button
      >
    </header>
  </section>
  {#if layout.showDetails}
    <Splitter bind:width={layout.details} side="right" min={240} max={560} />
    <aside class="details" style:width="{layout.details}px"></aside>
  {/if}
</main>

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

  main {
    display: flex;
    height: 100vh;
  }

  .sidebar {
    flex: none;
    background: var(--sidebar);
    border-right: 1px solid var(--line);
  }

  .library {
    flex: 1;
    min-width: 320px;
    display: flex;
    flex-direction: column;
  }

  header {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 44px;
    padding: 0 12px;
    border-bottom: 1px solid var(--line);
  }
  .title {
    flex: 1;
    font-weight: 600;
  }
  header button {
    padding: 4px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
  }
  header button.on {
    background: var(--line);
  }

  .details {
    flex: none;
    border-left: 1px solid var(--line);
  }
</style>
