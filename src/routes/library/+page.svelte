<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { ask, save } from "@tauri-apps/plugin-dialog";
  import { onMount, tick, untrack } from "svelte";
  import "$lib/theme.css";
  import { label, mac, matches, zoomStep, type Shortcut } from "$lib/keys";
  import { hasTag, renameTagFilter, toggleTag } from "$lib/query";
  import { foldTag, movedTag, sameTag } from "$lib/tags";
  import Details from "$lib/library/Details.svelte";
  import Add from "$lib/library/Add.svelte";
  import PapersMissing from "$lib/library/PapersMissing.svelte";
  import { Editor } from "$lib/library/editor.svelte";
  import { addBibtex, errorText, renameTag, replaceEntry, setTags, writeError, writers } from "$lib/library/writes";
  import { NativeReader } from "$lib/library/native.svelte";
  import { PdfjsReader } from "$lib/library/pdfjs.svelte";
  import ReaderBar from "$lib/library/ReaderBar.svelte";
  import ReaderView from "$lib/library/ReaderView.svelte";
  import Sidebar, { type View } from "$lib/library/Sidebar.svelte";
  import Splitter from "$lib/library/Splitter.svelte";
  import Table, { DEFAULT_COLUMNS, value, type Shown, type Sort } from "$lib/library/Table.svelte";
  import type { AddOptions, Added, Item, Row, Status, TagCount, WriteError } from "$lib/types";

  /** What the window keeps between sessions (window.json, in the data folder
   *  of the app): the widths of the side panes, whether the details show,
   *  the columns and order of the table, the papers open and the page of
   *  each, the parent tags unfolded in the sidebar. */
  type Layout = {
    sidebar: number;
    details: number;
    showSidebar: boolean;
    showDetails: boolean;
    columns: Shown[];
    sort: Sort;
    open: string[];
    /** the paper in front; none: the library */
    active: string | null;
    pages: Record<string, number>;
    /** the parent tags unfolded in the sidebar, folded as `foldTag` does */
    openTags: string[];
  };
  const DEFAULTS: Layout = {
    sidebar: 230,
    details: 340,
    showSidebar: true,
    showDetails: true,
    columns: DEFAULT_COLUMNS,
    sort: null,
    open: [],
    active: null,
    pages: {},
    openTags: [],
  };

  let layout = $state<Layout>({ ...DEFAULTS });
  let loaded = false;
  let timer: ReturnType<typeof setTimeout>;

  let status = $state<Status>({ state: "loading" });
  let rows = $state<Row[]>([]);
  let tags = $state<TagCount[]>([]);
  /** the tags pinned to the keys 1–9 (index = key − 1); a setting */
  let pinned = $state<(string | null)[]>([]);
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
  /** the menu of the export, open */
  let exporting = $state(false);
  /** papers to install, to write a papers library: the version found, too old */
  let missing = $state<{ version: string | null } | null>(null);
  /** the sheet to add papers, open */
  let adding = $state(false);
  /** what was just done, said for a moment */
  let notice = $state<string | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout>;
  /** the BibTeX of the paper in the details, being edited */
  const editor = new Editor();
  /** after a paper is closed: the library, or the next paper open (a setting) */
  let afterClose: "library" | "next" = "library";

  /** The commands of the window, and their keys: in the menu of the app on
   *  macOS, where they also work while the PDF view has the keys; caught by
   *  the page elsewhere. */
  const commands: Record<string, Shortcut> = {
    close: { key: "w", mod: true },
    add: { key: "n", mod: true },
    find: { key: "f", mod: true },
    "zoom-in": { key: "=", mod: true },
    "zoom-out": { key: "-", mod: true },
    "zoom-fit": { key: "0", mod: true },
    sidebar: { key: "s", mod: true, alt: true },
    details: { key: "i", mod: true, alt: true },
    library: { key: "1", mod: true },
    settings: { key: ",", mod: true },
  };
  const allShortcut: Shortcut = { key: "a", mod: true };

  const collator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });

  let byId = $derived(new Map(rows.map((r) => [r.id, r])));

  /** The papers the tags in the details change: the one in front, else the
   *  selection when the focus is in it, else the paper shown. */
  let targets = $derived.by(() => {
    const ids = layout.active ? [layout.active] : focus && selected.has(focus) ? [...selected] : item ? [item.id] : [];
    return ids.map((id) => byId.get(id)).filter((r): r is Row => r !== undefined);
  });

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

  let papers = $derived(layout.open.map((id) => byId.get(id)).filter((r): r is Row => r !== undefined));

  /** the PDF of the paper in front */
  let reader = $derived.by(() => {
    const id = layout.active;
    if (!id) return null;
    // PDFKit on macOS, pdf.js elsewhere
    const Kind = mac ? NativeReader : PdfjsReader;
    return untrack(() => new Kind(id, layout.pages[id] ?? 1, (page) => (layout.pages[id] = page)));
  });

  /** what the search found, next to it */
  let summary = $derived(
    `${shown.length} ${shown.length === 1 ? "paper" : "papers"}` +
      (selected.size ? ` · ${selected.size} selected` : ""),
  );

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
    const empty = (v: string | number | null) => v === null || v === "";
    const sign = sort.descending ? -1 : 1;
    return [...list].sort((a, b) => {
      const [x, y] = [value(a, sort.column), value(b, sort.column)];
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
      // papers opened from the panel meanwhile, settings changed
      window.onFocusChanged(({ payload: focused }) => {
        if (focused) refresh();
      }),
      // ⌘I in the panel
      listen<string>("open", (e) => openPaper(e.payload)),
      // from the menu of the app
      listen<string>("command", (e) => command(e.payload)),
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
    await refresh();
    // a paper to open with the window ("library#ID")
    if (location.hash) openPaper(decodeURIComponent(location.hash.slice(1)));
    await tick();
    const window = getCurrentWindow();
    await window.show();
    await window.setFocus();
    if (!layout.active) input?.focus();
  }

  async function refresh() {
    recent = await invoke<string[]>("recent");
    const { settings } = await invoke<{
      settings: { after_close: "library" | "next"; pinned_tags: (string | null)[] };
    }>("get_settings");
    afterClose = settings.after_close;
    pinned = settings.pinned_tags;
  }

  /** Pin tags to the keys, kept in the settings. */
  async function savePinned(tags: (string | null)[]) {
    while (tags.length && tags[tags.length - 1] === null) tags = tags.slice(0, -1);
    try {
      await invoke("set_pinned_tags", { tags });
      pinned = tags;
    } catch (err) {
      say(String(err));
    }
  }

  /** Pin a tag to the key of a slot, in place of the one there; or unpin it. */
  function pin(tag: string, slot: number | null) {
    const next = Array.from({ length: 9 }, (_, i) => pinned[i] ?? null).map((t) => (t && sameTag(t, tag) ? null : t));
    if (slot !== null) next[slot] = tag;
    savePinned(next);
  }

  /** Unfold a parent tag in the sidebar, or fold it. */
  function fold(tag: string, open: boolean) {
    const rest = layout.openTags.filter((t) => t !== foldTag(tag));
    layout.openTags = open ? [...rest, foldTag(tag)] : rest;
  }

  /** Rename a tag in the library, and the tags under it; the filters of the search, the keys they are pinned to
   *  and the parents unfolded follow. */
  async function rename(tag: string, name: string) {
    if (!(await write(() => renameTag(tag, name)))) return;
    query = renameTagFilter(query, tag, name);
    layout.openTags = layout.openTags.map((t) => foldTag(movedTag(t, tag, name)));
    const next = pinned.map((t) => t && movedTag(t, tag, name));
    // renamed to a tag pinned already: it keeps the first key
    const kept = next.map((t, i) => (t && next.findIndex((u) => u && sameTag(u, t)) < i ? null : t));
    if (kept.some((t, i) => t !== (pinned[i] ?? null))) await savePinned(kept);
  }

  /** The key of a pinned tag: the tag on the selected papers, else the one in focus; off
   *  when they all have it. */
  async function togglePinned(slot: number) {
    const tag = pinned[slot];
    const ids = selected.size ? [...selected] : focus ? [focus] : [];
    if (!tag || !ids.length) return;
    const count = (n: number) => `${n} ${n === 1 ? "paper" : "papers"}`;
    const lacking = ids.filter((id) => !byId.get(id)?.tags.some((t) => sameTag(t, tag)));
    if (lacking.length) {
      if (await write(() => setTags(lacking, [tag]))) say(`Added ${tag} to ${count(lacking.length)}`);
    } else if (await write(() => setTags(ids, [], [tag]))) {
      say(`Removed ${tag} from ${count(ids.length)}`);
    }
  }

  /** The library, at start and when its files change. */
  async function loadLibrary() {
    [rows, tags, recent, duplicates] = await Promise.all([
      invoke<Row[]>("rows"),
      invoke<TagCount[]>("tags"),
      invoke<string[]>("recent"),
      invoke<string[][]>("duplicates"),
    ]);
    // the selection and the tabs keep the papers still there
    selected = new Set([...selected].filter((id) => byId.has(id)));
    if (focus && !byId.has(focus)) focus = null;
    layout.open = layout.open.filter((id) => byId.has(id));
    if (layout.active && !byId.has(layout.active)) layout.active = null;
    if (editor.id && !byId.has(editor.id)) editor.stop();
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

  // the details of the paper in front, else of the one in focus; again
  // when the library reloads
  $effect(() => {
    const id = layout.active ?? focus;
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

  async function select(id: string, e: MouseEvent) {
    if (e.button !== 0 || !(await leave(id))) return;
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

  /** Move the focus by some rows, or to the first or last; extending the
   *  selection, or not. */
  async function move(to: number | "first" | "last", extend: boolean) {
    const ids = shown.map((r) => r.id);
    if (!ids.length) return;
    const at = focus ? ids.indexOf(focus) : -1;
    const index =
      to === "first" ? 0 : to === "last" ? ids.length - 1 : at < 0 ? 0 : Math.max(0, Math.min(ids.length - 1, at + to));
    const next = ids[index];
    if (!(await leave(next))) return;
    if (extend && anchor) {
      selectRange(anchor, next);
    } else {
      selected = new Set([next]);
      anchor = next;
    }
    focus = next;
  }

  /** Enter, a double-click: the paper in a tab, unless it has no file (its
   *  web page then opens in the browser). */
  async function open(id: string) {
    if (byId.get(id)?.file === "none") {
      await invoke("open", { id });
      recent = await invoke<string[]>("recent");
    } else {
      openPaper(id);
    }
  }

  async function openPaper(id: string) {
    if (!(await leave(id))) return;
    if (!layout.open.includes(id)) layout.open = [...layout.open, id];
    layout.active = id;
    invoke("opened", { id }).then(refresh);
  }

  /** The papers in a .bib file of their own, e.g. a manuscript's references. */
  async function exportBib(ids: string[]) {
    exporting = false;
    const path = await save({
      title: "Export as a .bib file",
      defaultPath: "references.bib",
      filters: [{ name: "BibTeX", extensions: ["bib"] }],
    });
    if (!path) return;
    try {
      const count = await invoke<number>("export_bib", { ids, path });
      say(`Exported ${count} ${count === 1 ? "paper" : "papers"} to ${path.split(/[\\/]/).pop()}`);
    } catch (err) {
      say(String(err));
    }
  }

  /** Run a write: its error said (by `fail`: as a notice), or how to install
   *  papers when a papers library cannot be written without it. The library
   *  is read again after a write (a "status" event). Whether it was written. */
  async function write(action: () => Promise<unknown>, fail = (error: WriteError) => say(error.message)): Promise<boolean> {
    try {
      await action();
      return true;
    } catch (e) {
      const error = writeError(e);
      if (error.kind === "papers-missing") {
        const found = (await writers()).find((w) => w.kind === "papers-missing");
        missing = { version: found?.version ?? null };
      } else {
        fail(error);
      }
      return false;
    }
  }

  /** The papers of BibTeX added to a .bib file. Then the sheet closes, and
   *  the first paper added (else the first that was there) is selected, in
   *  the library as it is without search or view to hide it. */
  async function addPapers(bib: string, text: string, options: AddOptions, fail: (message: string) => void): Promise<boolean> {
    let added: Added | undefined;
    if (!(await write(async () => (added = await addBibtex(bib, text, options)), (error) => fail(errorText(error)))) || !added)
      return false;
    adding = false;
    const keys = added.added.length ? added.added : added.existing;
    const [id] = await invoke<string[]>("ids_of", { bib, keys });
    if (id) {
      query = "";
      view = "all";
      await loadLibrary();
      selected = new Set([id]);
      anchor = focus = id;
      showLibrary();
    }
    const n = added.added.length;
    say(
      n
        ? `Added ${n} ${n === 1 ? "paper" : "papers"}` + (added.existing.length ? `, ${added.existing.length} already there` : "")
        : added.existing.length
          ? `Already in the library: ${added.existing.join(", ")}`
          : "Nothing added",
    );
    return true;
  }

  function say(text: string) {
    notice = text;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = null), 4000);
  }

  /** A paper closed: the library comes to the front, or the next paper open. */
  async function closePaper(id: string) {
    const at = layout.open.indexOf(id);
    const rest = layout.open.filter((x) => x !== id);
    const next = layout.active !== id ? layout.active : afterClose === "next" ? (rest[at] ?? rest[at - 1] ?? null) : null;
    // the edit of the paper goes with its tab
    if (!(await leave(editor.id === id ? null : (next ?? focus)))) return;
    layout.open = rest;
    delete layout.pages[id];
    if (layout.active === id) layout.active = next;
  }

  /** ⌘W: the paper in front, else the window. */
  async function closeActive() {
    if (layout.active) closePaper(layout.active);
    else if (await leave(null)) getCurrentWindow().close();
  }

  async function showLibrary() {
    if (!(await leave(focus))) return;
    layout.active = null;
    tick().then(() => input?.focus());
  }

  /** Whether to go on to show another paper (none: no paper) than the one edited, which is left: after asking
   *  when its BibTeX has changes. */
  async function leave(next: string | null): Promise<boolean> {
    const id = editor.id;
    if (id === null || id === next) return true;
    if (editor.changed) {
      const message = `Discard the changes to ${byId.get(id)?.key ?? id}?`;
      if (!(await ask(message, { title: "Edit", kind: "warning", okLabel: "Discard", cancelLabel: "Keep editing" }))) return false;
    }
    editor.stop();
    return true;
  }

  /** Edit the BibTeX of the paper in the details. */
  async function edit(id: string) {
    try {
      await editor.start(id);
    } catch (err) {
      say(String(err));
    }
  }

  /** ⌘S in the editor: the entry replaced in its .bib file; a refusal stays under the editor, with its text. */
  async function saveEdit() {
    const id = editor.id;
    if (id === null || !editor.changed || editor.saving) return;
    editor.saving = true;
    editor.error = null;
    const saved = await write(() => replaceEntry(id, editor.text), (error) => editor.fail(error));
    editor.saving = false;
    if (!saved) return;
    editor.stop();
    say(`Saved ${byId.get(id)?.key ?? id}`);
  }

  /** A command of the window, from the menu of the app or its keys. */
  function command(name: string) {
    switch (name) {
      case "close":
        return closeActive();
      case "add":
        adding = true;
        return;
      case "find":
        return reader ? reader.focusFind() : input?.select();
      // the PDF in front has the keys; else they set the text size
      case "zoom-in":
        return reader ? reader.zoom(1) : invoke("text_size_step", { steps: 1 });
      case "zoom-out":
        return reader ? reader.zoom(-1) : invoke("text_size_step", { steps: -1 });
      case "zoom-fit":
        return reader ? reader.zoom(0) : invoke("text_size_step", { steps: 0 });
      case "sidebar":
        layout.showSidebar = !layout.showSidebar;
        return;
      case "details":
        layout.showDetails = !layout.showDetails;
        return;
      case "library":
        return showLibrary();
      case "next-tab":
        return cycle(1);
      case "previous-tab":
        return cycle(-1);
      case "settings":
        invoke("open_settings");
    }
  }

  /** the rows a page key moves: about those in view */
  const pageRows = () => Math.max(1, Math.floor((window.innerHeight - 120) / 26));

  function onkeydown(e: KeyboardEvent) {
    // ⌃Tab: the menu of macOS does not get it from a field of the page
    if (e.ctrlKey && e.key === "Tab") {
      e.preventDefault();
      command(e.shiftKey ? "previous-tab" : "next-tab");
      return;
    }
    // ⌘+ also on a keyboard where it is ⌘⇧=
    const zoom = zoomStep(e);
    if (!mac && zoom !== null) {
      e.preventDefault();
      command(zoom > 0 ? "zoom-in" : zoom < 0 ? "zoom-out" : "zoom-fit");
      return;
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
    // a paper in front has the keys of its reader, the editor of BibTeX its own
    if (!layout.active && !(e.target instanceof HTMLTextAreaElement)) libraryKey(e);
  }

  /** The keys of the library: / to search, the arrows, the page keys, Home
   *  and End move (shift extends the selection), Enter opens, Esc clears the
   *  search then leaves it, 1–9 toggle the tag pinned to the key. */
  function libraryKey(e: KeyboardEvent) {
    const typing = e.target instanceof HTMLInputElement;
    const mod = mac ? e.metaKey : e.ctrlKey;
    const go = (to: number | "first" | "last") => {
      e.preventDefault();
      move(to, e.shiftKey);
    };
    if (e.key === "/" && !typing) {
      e.preventDefault();
      input?.select();
    } else if (e.key === "ArrowDown") {
      go(mod ? "last" : 1);
    } else if (e.key === "ArrowUp") {
      go(mod ? "first" : -1);
    } else if (e.key === "PageDown") {
      go(pageRows());
    } else if (e.key === "PageUp") {
      go(-pageRows());
    } else if (e.key === "Home" && !typing) {
      go("first");
    } else if (e.key === "End" && !typing) {
      go("last");
    } else if (e.key === "Enter" && focus) {
      e.preventDefault();
      open(focus);
    } else if (e.key === "Escape" && typing) {
      e.preventDefault();
      if (query) query = "";
      else input?.blur();
    } else if (/^[1-9]$/.test(e.key) && !typing && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
      e.preventDefault();
      togglePinned(Number(e.key) - 1);
    } else if (matches(e, allShortcut) && !typing) {
      e.preventDefault();
      selected = new Set(shown.map((r) => r.id));
    }
  }

  /** A tab to the front. */
  async function activate(id: string) {
    if (await leave(id)) layout.active = id;
  }

  /** ⌃Tab: the next tab, the library first; ⌃⇧Tab: the one before. */
  function cycle(step: number) {
    const tabs = [null, ...layout.open];
    const at = tabs.indexOf(layout.active);
    const next = tabs[(at + step + tabs.length) % tabs.length];
    if (next) activate(next);
    else showLibrary();
  }
</script>

<svelte:window {onkeydown} />

{#if missing}
  <PapersMissing version={missing.version} onclose={() => (missing = null)} />
{/if}
{#if adding}
  <Add bib={focus ? byId.get(focus)?.bib : null} rows={byId} onadd={addPapers} onclose={() => (adding = false)} />
{/if}

<main>
  {#if layout.showSidebar}
    <aside class="sidebar" style:width="{layout.sidebar}px">
      <Sidebar
        {libraries}
        {views}
        {tags}
        view={layout.active ? null : view}
        filtered={(tag) => !layout.active && hasTag(query, tag)}
        {pinned}
        open={layout.openTags}
        {papers}
        active={layout.active}
        onview={(v) => {
          view = v;
          showLibrary();
        }}
        ontag={(tag) => {
          query = toggleTag(query, tag);
          showLibrary();
        }}
        onfold={fold}
        onrename={rename}
        onpin={pin}
        dirty={editor.changed ? editor.id : null}
        onpaper={activate}
        onclose={closePaper}
      />
    </aside>
    <Splitter bind:width={layout.sidebar} side="left" min={160} max={420} />
  {/if}
  <div class="right">
    <header>
      {#if reader}
        <ReaderBar {reader} onclose={closeActive} />
      {:else}
        <input
          bind:this={input}
          bind:value={query}
          type="search"
          placeholder="Search: words, author:, year:2010-2015, tag:, has:pdf, -exclude"
          spellcheck="false"
          autocomplete="off"
        />
        <span class="count">{notice ?? summary}</span>
        <div class="export">
          <button class="tool" class:on={exporting} onclick={() => (exporting = !exporting)}>Export…</button>
          {#if exporting}
            <button class="backdrop" aria-label="Close" onclick={() => (exporting = false)}></button>
            <div class="menu" role="menu">
              <button role="menuitem" disabled={!selected.size} onclick={() => exportBib([...selected])}>
                The {selected.size} selected
              </button>
              <button role="menuitem" onclick={() => exportBib(shown.map((r) => r.id))}>The {shown.length} shown</button>
            </div>
          {/if}
        </div>
      {/if}
      <span class="spacer"></span>
      <button class="tool" title="Add papers ({label(commands.add)})" onclick={() => (adding = true)}>Add…</button>
      <button
        class="tool"
        class:on={layout.showDetails}
        title="Details ({label(commands.details)})"
        onclick={() => (layout.showDetails = !layout.showDetails)}>Details</button
      >
    </header>
    <div class="content">
      <section class="library">
        {#if reader}
          {#key reader}<ReaderView {reader} />{/key}
        {:else if status.state === "loading" && !rows.length}
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
            columns={layout.columns}
            {selected}
            {focus}
            sort={layout.sort}
            oncolumns={(columns) => (layout.columns = columns)}
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
            <Details
              {item}
              rows={targets}
              {tags}
              filtered={(tag) => !layout.active && hasTag(query, tag)}
              ontag={(tag) => {
                query = toggleTag(query, tag);
                showLibrary();
              }}
              onset={(add, remove) => write(() => setTags(targets.map((r) => r.id), add, remove))}
              onopen={() => open(item!.id)}
              {editor}
              onedit={() => edit(item!.id)}
              onsave={saveEdit}
              oncancel={() => leave(null)}
            />
          {:else}
            <p class="empty">No paper selected</p>
          {/if}
        </aside>
      {/if}
    </div>
  </div>
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

  .right {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .content {
    flex: 1;
    min-height: 0;
    display: flex;
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
  /* the search keeps its width: what is next to it stays in place */
  input {
    flex: 0 1 50%;
    min-width: 200px;
    max-width: 560px;
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
  /* room for "99999 papers · 999 selected": the export stays in place */
  .count {
    min-width: 23ch;
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .export {
    position: relative;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 2;
    border: none;
    background: none;
  }
  .menu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 3;
    display: flex;
    flex-direction: column;
    min-width: 160px;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--menu-bg);
  }
  .menu button {
    padding: 5px 10px;
    border: none;
    border-radius: 5px;
    background: none;
    color: var(--fg);
    font: inherit;
    text-align: left;
    white-space: nowrap;
  }
  .menu button:hover:not(:disabled) {
    background: var(--selection);
    color: var(--on-selection);
  }
  .menu button:disabled {
    color: var(--muted);
  }
  .spacer {
    flex: 1;
  }
  .tool {
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
  }
  .tool.on {
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
