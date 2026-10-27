<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, tick } from "svelte";
  import "$lib/theme.css";
  import { label, mac, matches, type Shortcut } from "$lib/keys";
  import { hasTag, toggleTag } from "$lib/query";
  import Details from "$lib/library/Details.svelte";
  import Sidebar, { type View } from "$lib/library/Sidebar.svelte";
  import Splitter from "$lib/library/Splitter.svelte";
  import Table, { type Sort } from "$lib/library/Table.svelte";
  import type { Item, Row, Status, TagCount } from "$lib/types";

  /** What the window keeps between sessions (window.json, in the data folder
   *  of the app): the widths of the side panes, whether the details show,
   *  the order of the table. */
  type Layout = { sidebar: number; details: number; showDetails: boolean; sort: Sort };
  const DEFAULTS: Layout = { sidebar: 230, details: 340, showDetails: true, sort: null };

  let layout = $state<Layout>({ ...DEFAULTS });
  let loaded = false;
  let timer: ReturnType<typeof setTimeout>;

  let status = $state<Status>({ state: "loading" });
  let rows = $state<Row[]>([]);
  let tags = $state<TagCount[]>([]);
  /** the recent papers, the last first */
  let recent = $state<string[]>([]);
  /** the groups of papers that may be the same */
  let duplicates = $state<string[][]>([]);

  let query = $state("");
  /** the papers that match the query; none until the first answer */
  let matched = $state<Set<string> | null>(null);
  let view = $state<View>("all");
  let selected = $state(new Set<string>());
  /** the paper the details show, and that the arrow keys move from */
  let focus = $state<string | null>(null);
  /** where shift extends the selection from */
  let anchor: string | null = null;
  let item = $state<Item | null>(null);
  let input = $state<HTMLInputElement>();

  const detailsShortcut: Shortcut = { key: "i", mod: true, alt: true };
  const settingsShortcut: Shortcut = { key: ",", mod: true };
  const findShortcut: Shortcut = { key: "f", mod: true };
  const allShortcut: Shortcut = { key: "a", mod: true };

  const collator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });

  let byId = $derived(new Map(rows.map((r) => [r.id, r])));

  let shown = $derived.by(() => {
    const m = matched;
    if (!m) return [];
    const pick = (ids: string[]) =>
      ids.map((id) => byId.get(id)).filter((r): r is Row => r !== undefined && m.has(r.id));
    let list: Row[];
    if (view === "recent") list = pick(recent);
    else if (view === "duplicates") list = pick(duplicates.flat());
    else list = rows.filter((r) => m.has(r.id) && inView(r, view));
    return layout.sort ? sorted(list, layout.sort) : list;
  });

  let views = $derived<{ view: View; name: string; count: number }[]>([
    { view: "all", name: "All papers", count: rows.length },
    { view: "recent", name: "Recently opened", count: recent.length },
    { view: "nopdf", name: "No PDF", count: rows.filter((r) => r.file === "none").length },
    { view: "broken", name: "Broken file links", count: rows.filter((r) => r.file === "missing").length },
    { view: "duplicates", name: "Possible duplicates", count: duplicates.flat().length },
  ]);

  let libraries = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const r of rows) counts.set(r.bib, (counts.get(r.bib) ?? 0) + 1);
    return [...counts].map(([bib, count]) => ({ bib, count }));
  });

  function inView(r: Row, v: View): boolean {
    if (typeof v !== "string") return r.bib === v.bib;
    if (v === "nopdf") return r.file === "none";
    if (v === "broken") return r.file === "missing";
    return true;
  }

  /** The rows in the order of a column; the empty values last. */
  function sorted(list: Row[], sort: NonNullable<Sort>): Row[] {
    const value = (r: Row) =>
      sort.column === "tags" ? r.tags.join(", ") : sort.column === "year" ? r.year : r[sort.column];
    const empty = (v: string | number | null) => v === null || v === "";
    const sign = sort.descending ? -1 : 1;
    return [...list].sort((a, b) => {
      const [x, y] = [value(a), value(b)];
      if (empty(x) || empty(y)) return Number(empty(x)) - Number(empty(y));
      return sign * (typeof x === "number" ? x - (y as number) : collator.compare(String(x), String(y)));
    });
  }

  /** The window is made hidden, not to flash empty: shown once drawn. */
  onMount(() => {
    const window = getCurrentWindow();
    const unlisten = [
      listen<Status>("status", (e) => {
        status = e.payload;
        if (status.state === "ready") loadLibrary();
      }),
      // papers opened from the panel meanwhile
      window.onFocusChanged(({ payload: focused }) => {
        if (focused) invoke<string[]>("recent").then((ids) => (recent = ids));
      }),
    ];
    load();
    return () => unlisten.forEach((u) => u.then((f) => f()));
  });

  async function load() {
    const saved = await invoke<Partial<Layout> | null>("layout");
    layout = { ...DEFAULTS, ...saved };
    loaded = true;
    status = await invoke<Status>("status");
    if (status.state === "ready") await loadLibrary();
    await tick();
    const window = getCurrentWindow();
    await window.show();
    await window.setFocus();
    input?.focus();
  }

  /** The library, at start and when its files change. */
  async function loadLibrary() {
    [rows, tags, recent, duplicates] = await Promise.all([
      invoke<Row[]>("rows"),
      invoke<TagCount[]>("tags"),
      invoke<string[]>("recent"),
      invoke<string[][]>("duplicates"),
    ]);
    // the selection keeps the papers still there
    selected = new Set([...selected].filter((id) => byId.has(id)));
    if (focus && !byId.has(focus)) focus = null;
    await search(query);
  }

  let searches = 0;

  async function search(q: string) {
    const n = ++searches;
    const ids = await invoke<string[]>("matching", { query: q });
    if (n === searches) matched = new Set(ids);
  }

  $effect(() => {
    search(query);
  });

  // saved a moment after a change: a pane dragged changes it many times
  $effect(() => {
    const snapshot = $state.snapshot(layout);
    if (!loaded) return;
    clearTimeout(timer);
    timer = setTimeout(() => invoke("save_layout", { layout: snapshot }), 300);
  });

  // the details of the paper in focus, again when the library reloads
  $effect(() => {
    const id = focus;
    rows;
    if (!id) {
      item = null;
      return;
    }
    let live = true;
    invoke<Item>("item", { id })
      .then((it) => live && (item = it))
      .catch(() => live && (item = null));
    return () => {
      live = false;
    };
  });

  function select(id: string, e: MouseEvent) {
    if (e.button !== 0) return;
    if (e.shiftKey && anchor) {
      selectRange(anchor, id);
    } else if (mac ? e.metaKey : e.ctrlKey) {
      const s = new Set(selected);
      if (s.has(id)) s.delete(id);
      else s.add(id);
      selected = s;
      anchor = id;
    } else {
      selected = new Set([id]);
      anchor = id;
    }
    focus = id;
  }

  function selectRange(from: string, to: string) {
    const ids = shown.map((r) => r.id);
    const [a, b] = [ids.indexOf(from), ids.indexOf(to)].sort((x, y) => x - y);
    selected = new Set(a < 0 ? [to] : ids.slice(a, b + 1));
  }

  /** ↑ ↓ move the focus; with shift they extend the selection. */
  function move(step: number, extend: boolean) {
    const ids = shown.map((r) => r.id);
    if (!ids.length) return;
    const at = focus ? ids.indexOf(focus) : -1;
    const next = ids[at < 0 ? 0 : Math.max(0, Math.min(ids.length - 1, at + step))];
    if (extend && anchor) {
      selectRange(anchor, next);
    } else {
      selected = new Set([next]);
      anchor = next;
    }
    focus = next;
  }

  async function open(id: string) {
    await invoke("open", { id });
    recent = await invoke<string[]>("recent");
  }

  function onkeydown(e: KeyboardEvent) {
    const typing = e.target instanceof HTMLInputElement;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      move(e.key === "ArrowDown" ? 1 : -1, e.shiftKey);
    } else if (e.key === "Enter" && focus) {
      e.preventDefault();
      open(focus);
    } else if (e.key === "Escape" && typing && query) {
      e.preventDefault();
      query = "";
    } else if (matches(e, findShortcut)) {
      e.preventDefault();
      input?.select();
    } else if (matches(e, allShortcut) && !typing) {
      e.preventDefault();
      selected = new Set(shown.map((r) => r.id));
    } else if (matches(e, detailsShortcut)) {
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
  <aside class="sidebar" style:width="{layout.sidebar}px">
    <Sidebar
      {libraries}
      {views}
      {tags}
      {view}
      filtered={(tag) => hasTag(query, tag)}
      onview={(v) => (view = v)}
      ontag={(tag) => (query = toggleTag(query, tag))}
    />
  </aside>
  <Splitter bind:width={layout.sidebar} side="left" min={160} max={420} />
  <section class="library">
    <header>
      <input
        bind:this={input}
        bind:value={query}
        type="search"
        placeholder="Search: words, author:, year:2010-2015, tag:, has:pdf, -exclude"
        spellcheck="false"
        autocomplete="off"
      />
      <span class="count">{shown.length} {shown.length === 1 ? "paper" : "papers"}</span>
      <button
        class:on={layout.showDetails}
        title="Details ({label(detailsShortcut)})"
        onclick={() => (layout.showDetails = !layout.showDetails)}>Details</button
      >
    </header>
    {#if status.state === "loading" && !rows.length}
      <p class="empty">Loading the library…</p>
    {:else if status.state === "failed"}
      <p class="empty error">{status.error}</p>
    {:else if !rows.length}
      <p class="empty">
        No .bib file yet. <button class="link" onclick={() => invoke("open_settings")}>Choose them in Settings</button>
      </p>
    {:else if matched && !shown.length}
      <p class="empty">No paper matches.</p>
    {:else}
      <Table
        rows={shown}
        {selected}
        {focus}
        sort={layout.sort}
        onsort={(sort) => (layout.sort = sort)}
        onselect={select}
        onopen={open}
      />
    {/if}
  </section>
  {#if layout.showDetails}
    <Splitter bind:width={layout.details} side="right" min={260} max={600} />
    <aside class="details" style:width="{layout.details}px">
      {#if item}
        <Details {item} onopen={() => open(item!.id)} />
      {:else}
        <p class="empty">No paper selected</p>
      {/if}
    </aside>
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
    overflow-y: auto;
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
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 46px;
    padding: 0 12px;
    border-bottom: 1px solid var(--line);
  }
  input {
    flex: 1;
    min-width: 0;
    height: 28px;
    padding: 0 10px;
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
  .count {
    color: var(--muted);
    font-size: 12px;
    white-space: nowrap;
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
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--line);
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
