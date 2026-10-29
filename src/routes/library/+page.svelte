<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview, type DragDropEvent } from "@tauri-apps/api/webview";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { ask, save } from "@tauri-apps/plugin-dialog";
  import { onMount, tick, untrack } from "svelte";
  import "$lib/theme.css";
  import { label, mac, matches, zoomStep, type Shortcut } from "$lib/keys";
  import { renameTagFilter } from "$lib/query";
  import { foldTag, movedTag, sameTag } from "$lib/tags";
  import * as cite from "$lib/cite";
  import { copy as copyText } from "$lib/clipboard";
  import Details from "$lib/library/Details.svelte";
  import DeleteSheet from "$lib/library/DeleteSheet.svelte";
  import Add from "$lib/library/Add.svelte";
  import PapersMissing from "$lib/library/PapersMissing.svelte";
  import { Editor } from "$lib/library/editor.svelte";
  import {
    addBibtex,
    attachPdf,
    errorText,
    redo,
    removeEntries,
    renameTag,
    replaceEntry,
    setTags,
    transferEntries,
    undo,
    writeError,
    writers,
  } from "$lib/library/writes";
  import { showRowMenu, type RowActions } from "$lib/library/rowmenu";
  import {
    OPEN_ASK,
    count,
    deletedMessage,
    menuTags,
    nextAfter,
    openedMessage,
    transferMessage,
  } from "$lib/library/selection";
  import { NativeReader } from "$lib/library/native.svelte";
  import { PdfjsReader } from "$lib/library/pdfjs.svelte";
  import ReaderBar from "$lib/library/ReaderBar.svelte";
  import ReaderView from "$lib/library/ReaderView.svelte";
  import {
    NO_FILTERS,
    VIEWS,
    chips,
    currentSet,
    fileName,
    hasTag,
    libraryCounts,
    renameTags,
    restored,
    searchOf,
    tabFilters,
    tagCounts,
    toggleTag,
    viewCounts,
    without,
    type Chip,
    type Filters,
    type View,
  } from "$lib/library/filters";
  import Sidebar from "$lib/library/Sidebar.svelte";
  import { DROP_NEW, pdfDrag } from "$lib/library/drag.svelte";
  import Splitter from "$lib/library/Splitter.svelte";
  import Table, { DEFAULT_COLUMNS, value, type Shown, type Sort } from "$lib/library/Table.svelte";
  import type { AddOptions, Added, FilesMode, Item, Opened, Row, Status, TagCount, Transferred, WriteError } from "$lib/types";

  /** What the window keeps between sessions (window.json, in the data folder
   *  of the app): the widths of the side panes, whether the details show,
   *  the columns and order of the table, the library tab (its library, view,
   *  tags and search), the papers open and the page of each, the parent tags
   *  unfolded in the sidebar. */
  type Layout = {
    sidebar: number;
    details: number;
    showSidebar: boolean;
    showDetails: boolean;
    columns: Shown[];
    sort: Sort;
    /** the filters of the library tab: the current set it shows */
    library: Filters;
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
    library: NO_FILTERS,
    open: [],
    active: null,
    pages: {},
    openTags: [],
  };

  let layout = $state<Layout>({ ...DEFAULTS, library: restored(null) });
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

  /** the papers that match the search and the tags of the library tab; none until the first answer */
  let matched = $state<Set<string> | null>(null);
  /** where the table of the library tab is scrolled to, kept while a paper is in front (not state: nothing draws it) */
  const table = { scroll: 0 };
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
  /** the sheet to add a paper, open: for a PDF to add to the paper `id`, else a new paper; with a PDF to start with, or the chooser of one */
  let adding = $state<{ id: string | null; pdf: string | null; choose: boolean } | null>(null);
  let sheet = $state<ReturnType<typeof Add>>();
  /** a PDF is being dragged over the window: onto the paper that would get it (none: a new paper), or onto the sheet */
  let dragging = $state<{ id: string | null; sheet: boolean } | null>(null);
  /** what takes the PDF being dragged: from outside the window, or from the Downloads panel */
  let overlay = $derived(dragging ?? (pdfDrag.target ? { id: pdfDrag.target.id, sheet: false } : null));
  /** what was just done, said for a moment */
  let notice = $state<string | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout>;
  /** papers to delete, being asked about: the question of the sheet */
  let deleting = $state<{ papers: { id: string; key: string; files: string[] }[]; note: string } | null>(null);
  /** the .bib file last written by a delete, copy or move, which Undo takes back until another paper is chosen */
  let lastWrite: string | null = null;
  /** a click on one of several selected papers leaves them selected, for a drag or a double-click; this one gets
   *  the selection when the click is no more than that */
  let collapse: string | null = null;
  /** the BibTeX of the paper in the details, being edited */
  const editor = new Editor();
  /** after a paper is closed: the next paper open, or the library (a setting) */
  let afterClose: "library" | "next" = "next";

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
    "show-next-tab": { key: "]", code: "BracketRight", mod: true, shift: true },
    "show-previous-tab": { key: "[", code: "BracketLeft", mod: true, shift: true },
    settings: { key: ",", mod: true },
    // the library's, except in a field, where they are the field's (the menu of macOS has them)
    undo: { key: "z", mod: true },
    redo: { key: "z", mod: true, shift: true },
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

  /** the current set: the papers of the library tab, library × view × tags × search */
  let shown = $derived.by(() => {
    const list = currentSet(rows, matched, layout.library, recent, duplicates);
    return layout.sort ? sorted(list, layout.sort) : list;
  });

  /** the views, with their numbers within the library chosen */
  let views = $derived.by(() => {
    const counts = viewCounts(rows, recent, duplicates, layout.library.bib);
    return VIEWS.map(({ view, name }) => ({ view, name, count: counts[view] }));
  });

  /** the tags of the sidebar, counted within the library chosen */
  let sidebarTags = $derived(layout.library.bib ? tagCounts(rows.filter((r) => r.bib === layout.library.bib)) : tags);

  /** the filters on, as chips above the table */
  let filterChips = $derived(chips(layout.library));

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

  let libraries = $derived(libraryCounts(rows));

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
      // a PDF dropped on the window
      getCurrentWebview().onDragDropEvent((e) => drag(e.payload)),
    ];
    load();
    return () => unlisten.forEach((u) => u.then((f) => f()));
  });

  async function load() {
    const saved = await invoke<Partial<Layout> | null>("layout");
    layout = { ...DEFAULTS, ...saved, library: restored(saved?.library) };
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
    layout.library.query = renameTagFilter(layout.library.query, tag, name);
    layout.library.tags = renameTags(layout.library.tags, tag, name);
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
    await setTagOn(tag, ids, !ids.every((id) => byId.get(id)?.tags.some((t) => sameTag(t, tag))));
  }

  /** A tag on papers, or off them. */
  async function setTagOn(tag: string, ids: string[], on: boolean) {
    if (on) {
      const lacking = ids.filter((id) => !byId.get(id)?.tags.some((t) => sameTag(t, tag)));
      if (lacking.length && (await write(() => setTags(lacking, [tag])))) say(`Added ${tag} to ${count(lacking.length)}`);
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
    // the library tab keeps the .bib file still there; the selection and the tabs, the papers
    if (layout.library.bib && rows.length && !rows.some((r) => r.bib === layout.library.bib)) layout.library.bib = null;
    selected = new Set([...selected].filter((id) => byId.has(id)));
    if (focus && !byId.has(focus)) focus = null;
    layout.open = layout.open.filter((id) => byId.has(id));
    if (layout.active && !byId.has(layout.active)) layout.active = null;
    if (editor.id && !byId.has(editor.id)) editor.stop();
    await search(searchOf(layout.library));
  }

  let searches = 0;

  async function search(q: string) {
    const n = ++searches;
    const ids = await invoke<string[]>("matching", { query: q });
    if (n === searches) matched = new Set(ids);
  }

  $effect(() => {
    search(searchOf(layout.library));
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
    lastWrite = null;
    collapse = null;
    if (e.shiftKey && anchor) {
      selectRange(anchor, id);
    } else if (mac ? e.metaKey : e.ctrlKey) {
      const s = new Set(selected);
      if (s.has(id)) s.delete(id);
      else s.add(id);
      selected = s;
      anchor = id;
    } else if (selected.has(id) && selected.size > 1) {
      collapse = id;
    } else {
      selected = new Set([id]);
      anchor = id;
    }
    focus = id;
  }

  /** The click that ends a press on one of several selected papers, which no drag followed: it selects that one. */
  function clickRow(id: string) {
    if (collapse === id) {
      selected = new Set([id]);
      anchor = id;
    }
    collapse = null;
  }

  /** The papers of `ids` that are in the table, in its order. */
  function ordered(ids: Iterable<string>): string[] {
    const set = new Set(ids);
    return shown.filter((r) => set.has(r.id)).map((r) => r.id);
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
    lastWrite = null;
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

  /** Whether to go on with an action on several papers: above `OPEN_ASK` it asks first. */
  async function confirmMany(n: number, question: string, button: string): Promise<boolean> {
    return n <= OPEN_ASK || (await ask(question, { title: button, okLabel: button, cancelLabel: "Cancel" }));
  }

  /** Enter, a double-click, Open: the papers each in a tab (above 5, after asking); one without a file opens its web
   *  page in the browser. The first in front. */
  async function openMany(ids: string[]) {
    if (!ids.length || !(await confirmMany(ids.length, `Open ${count(ids.length)}?`, "Open"))) return;
    const tabs = ids.filter((id) => byId.get(id)?.file !== "none");
    if (tabs.length) await openPapers(tabs);
    let failed = 0;
    for (const id of ids.filter((id) => !tabs.includes(id))) {
      try {
        await invoke("open", { id });
      } catch {
        failed++;
      }
    }
    if (tabs.length < ids.length) recent = await invoke<string[]>("recent");
    if (failed) say(`${count(failed)} without a file, DOI or URL`);
  }

  /** The PDFs of papers in the app that opens PDFs: Preview on macOS, else the default one. */
  async function openExternal(ids: string[]) {
    if (!(await confirmMany(ids.length, `Open ${count(ids.length)} outside the app?`, "Open"))) return;
    try {
      say(openedMessage(await invoke<Opened>("open_pdfs", { ids, preview: mac })));
      recent = await invoke<string[]>("recent");
    } catch (err) {
      say(String(err));
    }
  }

  /** The files of papers shown in the file manager. */
  async function revealMany(ids: string[]) {
    const found = ids.filter((id) => byId.get(id)?.file === "found");
    if (!(await confirmMany(found.length, `Show ${count(found.length)} in the ${mac ? "Finder" : "file manager"}?`, "Show"))) return;
    try {
      for (const id of found) await invoke("reveal", { id });
    } catch (err) {
      say(String(err));
    }
  }

  /** The web page of the DOI of papers. */
  async function openDois(ids: string[]) {
    const dois = ids.filter((id) => byId.get(id)?.doi);
    if (!(await confirmMany(dois.length, `Open ${dois.length} DOI pages?`, "Open"))) return;
    try {
      for (const id of dois) await invoke("open_link", { id });
    } catch (err) {
      say(String(err));
    }
  }

  /** The keys, or the BibTeX, of papers on the clipboard. */
  async function copyMany(ids: string[], format: "pandoc" | "bibtex") {
    try {
      const text = await invoke<string>("copy_many", { ids, format });
      say(
        format === "bibtex"
          ? `Copied the BibTeX of ${count(ids.length)}`
          : `Copied ${text.length > 50 ? text.slice(0, 50) + "…" : text}`,
      );
    } catch (err) {
      say(String(err));
    }
  }

  /** The citations of papers on the clipboard, one to a line. */
  async function copyCitations(ids: string[]) {
    try {
      const parts = await Promise.all(ids.map(async (id) => cite.toCopy(await cite.citation(id))));
      await copyText(ids[0], { text: parts.map((p) => p.text).join("\n"), html: parts.map((p) => p.html).join("<br>") });
      say(`Copied ${count(ids.length, "citation")}`);
    } catch (err) {
      say(String(err));
    }
  }

  /** One paper alone selected, with the details shown. Whether it was: a BibTeX being edited asks first. */
  async function showOnly(id: string): Promise<boolean> {
    if (!(await leave(id))) return false;
    selected = new Set([id]);
    anchor = focus = id;
    layout.showDetails = true;
    await tick();
    return true;
  }

  /** Edit BibTeX of the menu: the paper selected, its BibTeX in the details to edit. */
  async function editBibtex(id: string) {
    if (await showOnly(id)) await edit(id);
  }

  /** Edit tags… of the menu: the papers selected, the field of the tags in the details open. */
  async function editTags(ids: string[]) {
    layout.showDetails = true;
    selected = new Set(ids);
    focus = focus && ids.includes(focus) ? focus : ids[0];
    await tick();
    document.querySelector<HTMLButtonElement>("button[data-add-tag]")?.click();
  }

  /** Papers copied to another .bib file, or moved: what it did said, and after a move the next paper selected. */
  async function transfer(ids: string[], bib: string, move: boolean) {
    let done: Transferred | undefined;
    if (!(await write(async () => (done = await transferEntries(ids, bib, move)))) || !done) return;
    say(transferMessage(done, bib, move));
    const added = done.added;
    const moved = move ? ids.filter((id) => added.includes(byId.get(id)?.key ?? "")) : [];
    const sources = new Set(moved.map((id) => byId.get(id)?.bib));
    lastWrite = move ? (sources.size === 1 ? ([...sources][0] ?? null) : null) : bib;
    if (moved.length) {
      const next = nextAfter(shown, new Set(moved));
      selected = new Set(next ? [next] : []);
      anchor = focus = next;
    }
  }

  /** Delete… of the menu, ⌘⌫: the question first, which of their files to delete too. */
  async function askDelete(ids: string[]) {
    if (!ids.length) return;
    if (editor.id && ids.includes(editor.id) && !(await leave(null))) return;
    const kinds = new Map((await writers()).map((w) => [w.bib, w.kind]));
    const bibs = new Set(ids.map((id) => byId.get(id)?.bib ?? ""));
    const plain = [...bibs].some((b) => kinds.get(b) === "native");
    const papers = [...bibs].some((b) => kinds.get(b) !== "native");
    const note =
      papers && plain
        ? "In a papers library papers keeps the files in its history, so that Undo brings them back; those of a plain .bib go to the Trash."
        : papers
          ? "papers keeps them in its history, so that Undo brings them back."
          : "They go to the Trash.";
    const items = await Promise.all(ids.map((id) => invoke<Item>("item", { id })));
    deleting = { papers: items.map((it) => ({ id: it.id, key: it.key, files: it.files })), note };
  }

  /** The papers of the question deleted with the files chosen, then the next row selected. */
  async function doDelete(files: string[]) {
    if (!deleting) return;
    const ids = deleting.papers.map((p) => p.id);
    deleting = null;
    const next = nextAfter(shown, new Set(ids));
    const bibs = new Set(ids.map((id) => byId.get(id)?.bib));
    if (!(await write(() => removeEntries(ids, files)))) return;
    say(deletedMessage(ids.length, files.length));
    lastWrite = bibs.size === 1 ? ([...bibs][0] ?? null) : null;
    selected = new Set(next ? [next] : []);
    anchor = focus = next;
  }

  /** Right-click on a row: the papers selected when it is one of them, else that paper alone. */
  async function rowMenu(id: string, _e: MouseEvent) {
    if (!selected.has(id)) {
      if (!(await leave(id))) return;
      selected = new Set([id]);
      anchor = focus = id;
      lastWrite = null;
    }
    const picked = ordered(selected)
      .map((i) => byId.get(i))
      .filter((r): r is Row => r !== undefined);
    if (!picked.length) return;
    const bibs = (await writers()).map((w) => w.bib);
    await showRowMenu(picked, bibs, menuTags(pinned, tags), rowActions);
  }

  const rowActions: RowActions = {
    open: openMany,
    external: openExternal,
    reveal: revealMany,
    doi: openDois,
    copyKeys: (ids) => copyMany(ids, "pandoc"),
    copyBibtex: (ids) => copyMany(ids, "bibtex"),
    copyCitation: copyCitations,
    edit: editBibtex,
    tag: setTagOn,
    editTags,
    addPdf,
    transfer,
    exportBib,
    remove: askDelete,
  };

  /** ⌘Z, ⌘⇧Z: the last change of the .bib of the paper shown (of the one the app wrote last, after a delete, copy or
   *  move), else of the library tab's, else the only one. In a field, the field's own undo. */
  async function stepLibrary(back: boolean) {
    const el = document.activeElement;
    if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || (el instanceof HTMLElement && el.isContentEditable)) {
      document.execCommand(back ? "undo" : "redo");
      return;
    }
    const bib = lastWrite ?? byId.get(layout.active ?? focus ?? "")?.bib ?? layout.library.bib ?? (libraries.length === 1 ? libraries[0].bib : null);
    if (!bib) return say("Choose a paper or a library to " + (back ? "undo" : "redo") + " in");
    if (await write(() => (back ? undo(bib) : redo(bib)))) say(`${back ? "Undid" : "Redid"} the last change of ${fileName(bib)}`);
  }

  /** Papers in tabs, those not open yet after the ones open; `front` (the first, by default) comes to the front. */
  async function openPapers(ids: string[], front = ids[0]) {
    if (!ids.length || !(await leave(front))) return;
    layout.open = [...layout.open, ...ids.filter((id, i) => !layout.open.includes(id) && ids.indexOf(id) === i)];
    layout.active = front;
    Promise.all(ids.map((id) => invoke("opened", { id }))).then(refresh);
  }

  const openPaper = (id: string) => openPapers([id]);

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

  /** The paper selected, in the library tab, with its details: the filters are taken off if they hide it. */
  async function show(id: string) {
    await loadLibrary();
    if (!shown.some((r) => r.id === id)) layout.library = restored(null);
    selected = new Set([id]);
    anchor = focus = id;
    layout.showDetails = true;
    showLibrary();
  }

  /** A PDF dragged over the window or dropped on it: it opens the sheet, to add the paper it is of; dropped on the
   *  empty preview of a paper in the details, to add it to that paper; on the open sheet, it is the sheet's PDF. */
  function drag(e: DragDropEvent) {
    if (e.type === "leave") {
      dragging = null;
      return;
    }
    const ratio = window.devicePixelRatio;
    const under = document.elementFromPoint(e.position.x / ratio, e.position.y / ratio);
    const id = under?.closest<HTMLElement>("[data-drop-pdf]")?.dataset.dropPdf ?? null;
    if (e.type !== "drop") {
      dragging = { id, sheet: adding !== null };
      return;
    }
    dragging = null;
    const pdfs = e.paths.filter((p) => p.toLowerCase().endsWith(".pdf"));
    if (!pdfs.length) return say("Not a PDF");
    if (pdfs.length > 1) say("One PDF at a time: the first is used");
    if (sheet && adding) sheet.setPdf(pdfs[0]);
    else adding = { id, pdf: pdfs[0], choose: false };
  }

  /** The sheet, to add a PDF to a paper: with the chooser of the PDF open. */
  function addPdf(id: string) {
    adding = { id, pdf: null, choose: true };
  }

  /** The papers of BibTeX added to a .bib file. Then the sheet closes, and
   *  the first paper added (else the first that was there) is selected, in
   *  the library tab (see `show`). */
  async function addPapers(bib: string, text: string, options: AddOptions, fail: (message: string) => void): Promise<boolean> {
    let added: Added | undefined;
    if (!(await write(async () => (added = await addBibtex(bib, text, options)), (error) => fail(errorText(error)))) || !added)
      return false;
    adding = null;
    const keys = added.added.length ? added.added : added.existing;
    const [id] = await invoke<string[]>("ids_of", { bib, keys });
    if (id) await show(id);
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

  /** A paper changed: its entry takes `text` (the values chosen of an entry that is the same paper, or the changes
   *  of the edit), then the PDF is added to it, if there is one. Then the sheet closes and the paper is shown. */
  async function changePaper(
    id: string,
    text: string | null,
    pdf: string | null,
    files: FilesMode | null,
    fail: (message: string) => void,
  ): Promise<boolean> {
    const done = () => (pdf ? attachPdf(id, text, pdf, files) : replaceEntry(id, text ?? ""));
    if (!(await write(done, (error) => fail(errorText(error))))) return false;
    adding = null;
    const key = byId.get(id)?.key ?? id;
    await show(id);
    say(pdf ? `Added the PDF to ${key}` : `Merged into ${key}`);
    return true;
  }

  /** A paper of the library, from the link of the sheet that says an entry is the same paper. */
  async function showPaper(id: string) {
    adding = null;
    await show(id);
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
        adding = { id: null, pdf: null, choose: false };
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
      case "show-next-tab":
        return cycle(1);
      case "previous-tab":
      case "show-previous-tab":
        return cycle(-1);
      case "settings":
        invoke("open_settings");
        return;
      case "undo":
        return stepLibrary(true);
      case "redo":
        return stepLibrary(false);
    }
  }

  /** the rows a page key moves: about those in view */
  const pageRows = () => Math.max(1, Math.floor((window.innerHeight - 120) / 26));

  function onkeydown(e: KeyboardEvent) {
    // the question before a delete has the keys
    if (deleting) return;
    // ⌃Tab: the menu of macOS does not get it from a field of the page
    if (e.ctrlKey && e.key === "Tab") {
      e.preventDefault();
      command(e.shiftKey ? "previous-tab" : "next-tab");
      return;
    }
    // ⌘+ also on a keyboard where it is ⌘⇧=; on macOS ⌘+ is of the menu (which shows it), and ⌘= comes here
    const zoom = zoomStep(e);
    if ((!mac || (e.key === "=" && !e.shiftKey)) && zoom !== null) {
      e.preventDefault();
      command(zoom > 0 ? "zoom-in" : zoom < 0 ? "zoom-out" : "zoom-fit");
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
    // a paper in front has the keys of its reader, the editor of BibTeX its own
    if (!layout.active && !(e.target instanceof HTMLTextAreaElement)) libraryKey(e);
  }

  /** The keys of the library: / to search, ⌘⌫ clears the search in its field and deletes the papers selected
   *  elsewhere (after asking), the arrows, the page keys, Home
   *  and End move (shift extends the selection), Enter opens, Esc leaves the
   *  search, 1–9 toggle the tag pinned to the key. */
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
      openMany(selected.has(focus) ? ordered(selected) : [focus]);
    } else if (e.key === "Backspace" && mod && !e.shiftKey && !e.altKey && (e.target === input || !typing)) {
      e.preventDefault();
      // in the search field it clears the search; elsewhere it deletes the papers selected, if there are any
      const picked = ordered(selected);
      if (e.target !== input && picked.length) askDelete(picked);
      else layout.library.query = "";
    } else if (e.key === "Escape" && typing) {
      e.preventDefault();
      input?.blur();
    } else if (/^[1-9]$/.test(e.key) && !typing && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
      e.preventDefault();
      togglePinned(Number(e.key) - 1);
    } else if (matches(e, allShortcut) && !typing) {
      e.preventDefault();
      selected = new Set(shown.map((r) => r.id));
    }
  }

  /** Click on a tag: filter by it too, or not any more. */
  function filterByTag(tag: string) {
    layout.library.tags = toggleTag(layout.library.tags, tag);
    showLibrary();
  }

  function clearSearch() {
    layout.library.query = "";
    input?.focus();
  }

  /** The × of a chip. */
  function dropFilter(chip: Chip) {
    layout.library = without(layout.library, chip);
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
  <Add
    bind:this={sheet}
    bib={(focus ? byId.get(focus)?.bib : null) ?? layout.library.bib}
    rows={byId}
    forId={adding.id}
    pdf={adding.pdf}
    choose={adding.choose}
    onadd={addPapers}
    onchange={changePaper}
    onshow={showPaper}
    onclose={() => (adding = null)}
  />
{/if}
{#if deleting}
  <DeleteSheet
    papers={deleting.papers}
    note={deleting.note}
    ondelete={doDelete}
    oncancel={() => (deleting = null)}
  />
{/if}
{#if overlay}
  <div class="drop">
    <span>
      {overlay.sheet
        ? "Drop the PDF to use it"
        : overlay.id
          ? `Drop the PDF to add it to ${byId.get(overlay.id)?.key ?? "the paper"}`
          : "Drop the PDF to add a paper"}
    </span>
  </div>
{/if}

<main>
  {#if layout.showSidebar}
    <aside class="sidebar" style:width="{layout.sidebar}px">
      <Sidebar
        {libraries}
        bib={layout.library.bib}
        {views}
        tags={sidebarTags}
        view={layout.library.view}
        filtered={(tag) => hasTag(layout.library.tags, tag)}
        {pinned}
        open={layout.openTags}
        tab={{ filters: tabFilters(layout.library), count: shown.length }}
        {papers}
        active={layout.active}
        onlibrary={(bib) => {
          layout.library.bib = bib;
          showLibrary();
        }}
        onview={(v) => {
          layout.library.view = v;
          showLibrary();
        }}
        ontag={filterByTag}
        onlibrarytab={showLibrary}
        onfold={fold}
        onrename={rename}
        onpin={pin}
        dirty={editor.changed ? editor.id : null}
        onpaper={activate}
        onclose={closePaper}
        ondownload={(pdf, id) => (adding = { id, pdf, choose: false })}
      />
    </aside>
    <Splitter bind:width={layout.sidebar} side="left" min={160} max={420} />
  {/if}
  <div class="right">
    <header>
      {#if reader}
        <ReaderBar {reader} onclose={closeActive} />
      {:else}
        <div class="search">
          <input
            bind:this={input}
            bind:value={layout.library.query}
            type="search"
            placeholder="Search: words, author:, year:2010-2015, tag:, has:pdf, -exclude"
            spellcheck="false"
            autocomplete="off"
          />
          {#if layout.library.query}
            <button class="clear" title="Clear the search ({label({ key: 'Backspace', mod: true })})" aria-label="Clear the search" onclick={clearSearch}>×</button>
          {/if}
        </div>
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
      <button class="tool" title="Add a paper ({label(commands.add)}), or drop a PDF on the window"
        onclick={() => (adding = { id: null, pdf: null, choose: false })}>Add…</button
      >
      <button
        class="tool"
        class:on={layout.showDetails}
        title="Details ({label(commands.details)})"
        onclick={() => (layout.showDetails = !layout.showDetails)}>Details</button
      >
    </header>
    <div class="content">
      <section class="library" {...{ [DROP_NEW]: "" }}>
        {#if !reader && filterChips.length}
          <div class="chips">
            {#each filterChips as chip, i (chip.kind + chip.text)}
              {#if i}<span class="sep" aria-hidden="true">›</span>{/if}
              <span class="chip {chip.kind}">
                <span class="text" title={chip.text}>{chip.text}</span>
                <button aria-label="Remove the filter {chip.text}" onclick={() => dropFilter(chip)}>×</button>
              </span>
            {/each}
          </div>
        {/if}
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
            scroll={table.scroll}
            onscrolled={(top) => (table.scroll = top)}
            oncolumns={(columns) => (layout.columns = columns)}
            onsort={(sort) => (layout.sort = sort)}
            onselect={select}
            onclickrow={clickRow}
            onopen={openMany}
            onmenu={rowMenu}
            ondrop={openMany}
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
              filtered={(tag) => hasTag(layout.library.tags, tag)}
              ontag={filterByTag}
              onset={(add, remove) => write(() => setTags(targets.map((r) => r.id), add, remove))}
              onopen={() => open(item!.id)}
              {editor}
              onedit={() => edit(item!.id)}
              onaddpdf={() => addPdf(item!.id)}
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
  .search {
    position: relative;
    flex: 0 1 50%;
    min-width: 200px;
    max-width: 560px;
    display: flex;
  }
  input {
    flex: 1;
    min-width: 0;
    height: 28px;
    padding: 0 24px 0 10px;
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
  /* the × is the field's own, same on every platform */
  input::-webkit-search-cancel-button {
    display: none;
  }
  .clear {
    position: absolute;
    top: 4px;
    right: 4px;
    width: 20px;
    height: 20px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: none;
    color: var(--muted);
    font-size: 15px;
    line-height: 20px;
  }
  .clear:hover {
    background: var(--line);
    color: var(--fg);
  }
  /* the filters on: library › view › tags › search */
  .chips {
    flex: none;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 6px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--line);
    color: var(--muted);
    font-size: 12px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    max-width: 260px;
    padding: 1px 2px 1px 8px;
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--fg);
  }
  .chip .text {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .chip button {
    flex: none;
    width: 16px;
    height: 16px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: none;
    color: var(--muted);
    font-size: 13px;
    line-height: 16px;
  }
  .chip button:hover {
    background: var(--line);
    color: var(--fg);
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
  /* a PDF is over the window: the frame says where it would go */
  .drop {
    position: fixed;
    inset: 6px;
    z-index: 10;
    display: flex;
    align-items: flex-end;
    justify-content: center;
    box-sizing: border-box;
    padding-bottom: 18px;
    border: 2px dashed var(--accent);
    border-radius: 10px;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    pointer-events: none;
  }
  .drop span {
    padding: 4px 12px;
    border-radius: 6px;
    background: var(--accent);
    color: var(--on-accent);
    font-size: 12px;
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
