<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, tick } from "svelte";
  import { label, mac, matches, type Shortcut } from "$lib/keys";
  import type { Item, Status } from "$lib/types";

  type Action = {
    label: (it: Item) => string;
    shortcut: Shortcut;
    when?: (it: Item) => boolean;
    run: (it: Item) => Promise<void>;
  };

  let query = $state("");
  let items = $state<Item[]>([]);
  let selected = $state(0);
  let status = $state<Status>({ state: "loading" });
  let notice = $state<{ text: string; error: boolean } | null>(null);
  let menu = $state(false);
  let menuSelected = $state(0);
  let input: HTMLInputElement;
  let list = $state<HTMLUListElement>();

  const call = (command: string) => async (it: Item) => {
    await invoke(command, { key: it.key });
  };

  const copy = (format: string) => async (it: Item) => {
    const text = await invoke<string>("copy", { key: it.key, format });
    notice = { text: `Copied ${text.split("\n")[0]}`, error: false };
    setTimeout(() => invoke("hide_panel"), 400);
  };

  const hasLink = (it: Item) => !!(it.doi || it.url);

  const actions: Action[] = [
    {
      label: (it) => (it.files.length ? "Open PDF" : "Open DOI page"),
      shortcut: { key: "Enter" },
      when: (it) => it.files.length > 0 || hasLink(it),
      run: call("open"),
    },
    {
      label: () => (mac ? "Reveal in Finder" : "Show in folder"),
      shortcut: { key: "Enter", mod: true },
      when: (it) => it.files.length > 0,
      run: call("reveal"),
    },
    {
      label: () => "Open DOI page",
      shortcut: { key: "d", mod: true },
      when: hasLink,
      run: call("open_link"),
    },
    { label: () => "Copy key", shortcut: { key: "c", mod: true, shift: true }, run: copy("key") },
    { label: () => "Copy [@key]", shortcut: { key: ".", mod: true }, run: copy("pandoc") },
    { label: () => "Copy \\cite{key}", shortcut: { key: "l", mod: true }, run: copy("latex") },
    { label: () => "Copy BibTeX", shortcut: { key: "b", mod: true }, run: copy("bibtex") },
  ];
  const menuShortcut: Shortcut = { key: "k", mod: true };

  let current = $derived(items[selected]);
  let available = $derived(current ? actions.filter((a) => !a.when || a.when(current)) : []);

  async function run(action: Action) {
    const item = current;
    menu = false;
    if (!item || (action.when && !action.when(item))) return;
    try {
      await action.run(item);
    } catch (err) {
      notice = { text: String(err), error: true };
    }
  }

  async function search() {
    const q = query;
    const found = await invoke<Item[]>("search", { query: q });
    if (q !== query) return; // a newer search is on its way
    items = found;
    selected = 0;
    menu = false;
    notice = null;
    list?.scrollTo(0, 0);
  }

  async function select(i: number) {
    selected = Math.max(0, Math.min(items.length - 1, i));
    await tick();
    list?.children[selected]?.scrollIntoView({ block: "nearest" });
  }

  function move(e: KeyboardEvent): boolean {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const step = e.key === "ArrowDown" ? 1 : -1;
      if (menu) menuSelected = (menuSelected + step + available.length) % available.length;
      else select(selected + step);
      return true;
    }
    return false;
  }

  function onkeydown(e: KeyboardEvent) {
    if (move(e)) {
      e.preventDefault();
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (menu) menu = false;
      else invoke("hide_panel");
    } else if (matches(e, menuShortcut)) {
      e.preventDefault();
      menu = !menu && available.length > 0;
      menuSelected = 0;
    } else if (menu && e.key === "Enter") {
      e.preventDefault();
      run(available[menuSelected]);
    } else {
      const action = actions.find((a) => matches(e, a.shortcut));
      if (action) {
        e.preventDefault();
        run(action);
      }
    }
  }

  onMount(() => {
    invoke<Status>("status").then((s) => (status = s));
    const unlisten = [
      listen<Status>("status", (e) => {
        status = e.payload;
        if (status.state === "ready") search();
      }),
      listen("shown", () => {
        menu = false;
        notice = null;
        input.focus();
        input.select();
      }),
    ];
    return () => unlisten.forEach((u) => u.then((f) => f()));
  });
</script>

<svelte:window {onkeydown} />

<main class="panel">
  <input
    bind:this={input}
    bind:value={query}
    oninput={search}
    placeholder="Search papers"
    spellcheck="false"
    autocomplete="off"
  />

  {#if items.length}
    <ul bind:this={list} role="listbox">
      {#each items as item, i (item.key)}
        <!-- the keys are handled on the window -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          role="option"
          aria-selected={i === selected}
          tabindex="-1"
          class:selected={i === selected}
          onclick={() => select(i)}
          ondblclick={() => run(actions[0])}
        >
          <div class="text">
            <div class="title">{item.title}</div>
            <div class="subtitle">{item.subtitle}</div>
          </div>
          {#if item.files.length}
            <span class="badge">PDF</span>
          {:else if item.doi}
            <span class="badge">DOI</span>
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <div class="empty">
      {#if query.trim() && status.state === "ready"}No match{/if}
    </div>
  {/if}

  {#if menu && current}
    <div class="menu" role="menu">
      {#each available as action, i}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          role="menuitem"
          tabindex="-1"
          class:selected={i === menuSelected}
          onclick={() => run(action)}
          onmouseenter={() => (menuSelected = i)}
        >
          <span>{action.label(current)}</span>
          <kbd>{label(action.shortcut)}</kbd>
        </div>
      {/each}
    </div>
  {/if}

  <footer class:error={notice?.error || (!notice && status.state === "failed")}>
    <span class="status">
      {#if notice}
        {notice.text}
      {:else if status.state === "loading"}
        Loading library…
      {:else if status.state === "ready"}
        {status.count} papers
      {:else}
        {status.error}
      {/if}
    </span>
    {#if current && !notice}
      <span class="hints">
        {actions[0].label(current)} <kbd>{label(actions[0].shortcut)}</kbd>
        Actions <kbd>{label(menuShortcut)}</kbd>
      </span>
    {/if}
  </footer>
</main>

<style>
  :global(:root) {
    --bg: rgba(246, 246, 248, 0.98);
    --fg: #1d1d1f;
    --muted: #6e6e73;
    --border: rgba(0, 0, 0, 0.14);
    --line: rgba(0, 0, 0, 0.08);
    --menu-bg: #ffffff;
    --accent: #2f6fde;
    --on-accent: #ffffff;
    --error: #c4321f;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg: rgba(38, 38, 40, 0.98);
      --fg: #f2f2f4;
      --muted: #9a9aa0;
      --border: rgba(255, 255, 255, 0.16);
      --line: rgba(255, 255, 255, 0.08);
      --menu-bg: #323236;
      --accent: #3a78e6;
      --error: #ff7a66;
    }
  }
  :global(html, body) {
    margin: 0;
    height: 100%;
    overflow: hidden;
    background: transparent;
    color: var(--fg);
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    -webkit-user-select: none;
    user-select: none;
    cursor: default;
  }

  .panel {
    position: relative;
    box-sizing: border-box;
    height: 100vh;
    display: flex;
    flex-direction: column;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: hidden;
  }

  input {
    flex: none;
    padding: 14px 18px;
    border: none;
    border-bottom: 1px solid var(--line);
    outline: none;
    background: transparent;
    color: var(--fg);
    font: inherit;
    font-size: 20px;
  }
  input::placeholder {
    color: var(--muted);
  }

  ul {
    flex: 1;
    margin: 0;
    padding: 6px;
    list-style: none;
    overflow-y: auto;
  }
  li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 12px;
    border-radius: 7px;
  }
  li.selected,
  .menu .selected {
    background: var(--accent);
    color: var(--on-accent);
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  .title,
  .subtitle {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .title {
    font-size: 14px;
    font-weight: 500;
  }
  .subtitle {
    margin-top: 2px;
    font-size: 12px;
    color: var(--muted);
  }
  li.selected .subtitle {
    color: inherit;
    opacity: 0.85;
  }
  .badge {
    flex: none;
    padding: 1px 6px;
    border: 1px solid currentColor;
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    opacity: 0.55;
  }

  .empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--muted);
    font-size: 14px;
  }

  .menu {
    position: absolute;
    right: 10px;
    bottom: 36px;
    min-width: 240px;
    padding: 5px;
    background: var(--menu-bg);
    border: 1px solid var(--border);
    border-radius: 9px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.22);
    font-size: 13px;
  }
  .menu div {
    display: flex;
    justify-content: space-between;
    gap: 24px;
    padding: 6px 10px;
    border-radius: 6px;
  }

  kbd {
    font: inherit;
    font-size: 11px;
    opacity: 0.7;
  }

  footer {
    flex: none;
    display: flex;
    justify-content: space-between;
    gap: 16px;
    padding: 6px 18px;
    border-top: 1px solid var(--line);
    color: var(--muted);
    font-size: 11px;
  }
  footer .status {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    -webkit-user-select: text;
    user-select: text;
  }
  footer .hints {
    flex: none;
  }
  footer .hints kbd {
    margin: 0 8px 0 3px;
  }
  footer .hints kbd:last-child {
    margin-right: 0;
  }
  footer.error .status {
    color: var(--error);
    white-space: normal;
  }
</style>
