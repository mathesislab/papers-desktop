<script lang="ts">
  // The settings window: a bar of tabs (⌘1…⌘5, Ctrl elsewhere), the tab shown, which scrolls, and the line at its
  // foot. The tabs stay made while the window is open: each keeps what is typed in it, and where it was scrolled.
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, tick } from "svelte";
  import "$lib/theme.css";
  import { label, matches, zoomStep } from "$lib/keys";
  import AdvancedSettings from "$lib/settings/AdvancedSettings.svelte";
  import FeedsSettings from "$lib/settings/FeedsSettings.svelte";
  import GeneralSettings from "$lib/settings/GeneralSettings.svelte";
  import LibrarySettings from "$lib/settings/LibrarySettings.svelte";
  import SearchSettings from "$lib/settings/SearchSettings.svelte";
  import { Preferences } from "$lib/settings/settings.svelte";
  import { Updates } from "$lib/settings/updates.svelte";

  /** the tabs, in the order of the bar and of their keys; their ids are the sections `open_settings` names */
  const TABS = [
    { id: "library", name: "Library" },
    { id: "feeds", name: "Feeds" },
    { id: "search", name: "Search" },
    { id: "general", name: "General" },
    { id: "advanced", name: "Advanced" },
  ] as const;
  type Tab = (typeof TABS)[number]["id"];

  const prefs = new Preferences();
  const updates = new Updates();
  /** the tab shown: the last one while the window is open; Library at first */
  let tab = $state<Tab>("library");

  onMount(() => {
    load();
    const unlisten = listen<string>("section", (e) => showSection(e.payload));
    return () => unlisten.then((f) => f());
  });

  /** Fill the window, then show it: it is made hidden, not to flash empty.
   *  Its URL may name a section to show ("settings#updates"). */
  async function load() {
    await Promise.allSettled([prefs.load(), updates.load()]);
    await tick();
    const window = getCurrentWindow();
    await window.show();
    await window.setFocus();
    if (location.hash) showSection(location.hash.slice(1));
  }

  /** Show a section the app names: a tab, or "updates", those of the General tab, which also look for one unless
   *  found. */
  async function showSection(section: string) {
    if (section === "updates") {
      tab = "general";
      await tick();
      document.getElementById("updates")?.scrollIntoView({ block: "start" });
      updates.lookFor();
      return;
    }
    const named = TABS.find((t) => t.id === section);
    if (named) tab = named.id;
  }

  /** the key of the tab at `i`: its digit, whatever the layout types there */
  const shortcut = (i: number) => ({ key: String(i + 1), code: `Digit${i + 1}`, mod: true });

  /** ⌘W closes the window, ⌘+ and ⌘- change the text size, ⌘1…⌘5 show a tab. A hotkey being recorded takes the
   *  keys before (SearchSettings). */
  function onkeydown(e: KeyboardEvent) {
    if (matches(e, { key: "w", mod: true })) getCurrentWindow().close();
    const zoom = zoomStep(e);
    if (zoom !== null) {
      e.preventDefault();
      invoke("text_size_step", { steps: zoom });
    }
    const n = TABS.findIndex((_, i) => matches(e, shortcut(i)));
    if (n >= 0) {
      e.preventDefault();
      tab = TABS[n].id;
    }
  }
</script>

<svelte:window {onkeydown} />

{#if prefs.settings}
  <main class="settings">
    <div class="tabs" role="tablist" aria-label="Settings">
      {#each TABS as t, i (t.id)}
        <button
          role="tab"
          id="tab-{t.id}"
          aria-selected={tab === t.id}
          aria-controls="panel-{t.id}"
          class:on={tab === t.id}
          title={label(shortcut(i))}
          onclick={() => (tab = t.id)}>{t.name}</button
        >
      {/each}
    </div>
    {#each TABS as t (t.id)}
      <div class="panel" role="tabpanel" id="panel-{t.id}" aria-labelledby="tab-{t.id}" hidden={tab !== t.id}>
        {#if t.id === "library"}
          <LibrarySettings {prefs} />
        {:else if t.id === "feeds"}
          <FeedsSettings notify={prefs.show} />
        {:else if t.id === "search"}
          <SearchSettings {prefs} shown={tab === "search"} />
        {:else if t.id === "general"}
          <GeneralSettings {prefs} {updates} />
        {:else}
          <AdvancedSettings {prefs} />
        {/if}
      </div>
    {/each}
    <footer class:error={prefs.notice?.error}>{prefs.notice?.text ?? "Changes apply at once."}</footer>
  </main>
{/if}

<style>
  :global(html, body) {
    margin: 0;
    background: var(--bg);
    color: var(--fg);
    font-size: 13px;
    cursor: default;
    -webkit-user-select: none;
    user-select: none;
  }

  main {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  /* the bar of tabs: the one shown lifted, in the accent */
  .tabs {
    flex: none;
    display: flex;
    justify-content: center;
    gap: 2px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--line);
    background: var(--sidebar);
  }
  .tabs button {
    padding: 4px 12px;
    border: 1px solid transparent;
    border-radius: 6px;
    background: none;
    color: var(--muted);
  }
  .tabs button:hover {
    color: var(--fg);
  }
  .tabs button.on {
    border-color: var(--line);
    background: var(--menu-bg);
    color: var(--accent);
  }

  /* a tab: its cards, scrolled on their own */
  .panel {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 16px 22px;
  }
  .panel[hidden] {
    display: none;
  }

  footer {
    flex: none;
    padding: 8px 22px 10px;
    border-top: 1px solid var(--line);
    color: var(--muted);
    font-size: 11.5px;
    text-align: center;
  }
  footer.error {
    color: var(--error);
    -webkit-user-select: text;
    user-select: text;
  }
</style>
