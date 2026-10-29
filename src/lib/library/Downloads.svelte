<script lang="ts">
  // The Downloads panel at the bottom of the sidebar: the PDFs of a folder outside the library, to drag into it
  // (onto the table: a new paper; onto the empty preview of a paper: its PDF), or to open in the Add sheet with a
  // double-click. Only the names, sizes and dates of the files of that folder are read; nothing is sent anywhere.
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { Menu, MenuItem } from "@tauri-apps/api/menu";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { mac } from "$lib/keys";
  import { DRAG_DISTANCE, endPdfDrag, pdfDrag, pdfTarget } from "./drag.svelte";
  import { folderName, formatDate, formatSize, libraryNote, type Download, type DownloadsView } from "./downloads";

  let {
    onadd,
    onshow,
  }: {
    /** a PDF dropped on the table (id none: a new paper) or on the empty preview of a paper, or opened by a double-click */
    onadd: (path: string, id: string | null) => void;
    /** a paper of the library shown: the one that has a PDF of the panel */
    onshow: (id: string) => void;
  } = $props();

  let view = $state<DownloadsView | null>(null);
  /** why the panel could not do what was asked */
  let problem = $state<string | null>(null);

  async function load() {
    try {
      view = await invoke<DownloadsView>("downloads_list");
    } catch (err) {
      problem = String(err);
    }
  }

  onMount(() => {
    load();
    const unlisten = [
      // PDFs came or went, or the setting changed
      listen("downloads", load),
      // the library was read again: what is in it may have changed
      listen("status", (e) => (e.payload as { state: string }).state === "ready" && load()),
    ];
    return () => unlisten.forEach((u) => u.then((f) => f()));
  });

  const where = mac ? "Finder" : "file manager";
  const reveal = () => invoke("open_downloads").catch((err) => (problem = String(err)));

  async function chooseFolder() {
    const folder = await open({ directory: true, title: "The folder of the Downloads panel", defaultPath: view?.folder });
    if (typeof folder !== "string") return;
    problem = null;
    await invoke("set_downloads_folder", { folder }).catch((err) => (problem = String(err)));
  }

  /** The menu: from the button, or a right-click on the panel. */
  async function menu(e: MouseEvent) {
    e.preventDefault();
    const items = [
      await MenuItem.new({ text: "Choose Folder…", action: chooseFolder }),
      await MenuItem.new({ text: mac ? "Show in Finder" : "Open the Folder", action: reveal }),
      await MenuItem.new({
        text: "Hide",
        action: () => invoke("set_downloads_panel", { shown: false }).catch((err) => (problem = String(err))),
      }),
    ];
    await (await Menu.new({ items })).popup();
  }

  /** The right-click on a PDF in the library: show its paper (one item each, when several have it). */
  async function fileMenu(e: MouseEvent, file: Download) {
    if (!file.matches.length) return;
    e.preventDefault();
    e.stopPropagation();
    const items = await Promise.all(
      file.matches.map((m) => MenuItem.new({ text: `Show ${m.key} in the library`, action: () => onshow(m.id) })),
    );
    await (await Menu.new({ items })).popup();
  }

  /** A PDF pressed: dragged far enough, it is dragged, to drop on the table or on the empty preview of a paper;
   *  `Esc` cancels. */
  function press(e: PointerEvent, file: Download) {
    if (e.button !== 0 || e.ctrlKey) return;
    const [x, y] = [e.clientX, e.clientY];
    let started = false;
    const move = (m: PointerEvent) => {
      if (!started) {
        if (Math.hypot(m.clientX - x, m.clientY - y) < DRAG_DISTANCE) return;
        started = true;
        pdfDrag.path = file.path;
        pdfDrag.name = file.name;
      }
      [pdfDrag.x, pdfDrag.y] = [m.clientX, m.clientY];
      pdfDrag.target = pdfTarget(m.clientX, m.clientY);
    };
    const finish = (drop: boolean) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", key, true);
      if (!started) return;
      const target = drop ? pdfDrag.target : null;
      endPdfDrag();
      if (target) onadd(file.path, target.id);
    };
    const up = () => finish(true);
    const cancel = () => finish(false);
    const key = (k: KeyboardEvent) => {
      if (k.key !== "Escape") return;
      k.stopPropagation();
      finish(false);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", key, true);
  }
</script>

{#if view?.shown}
  <!-- not part of the library: a different background, a dashed edge -->
  <section class="downloads" aria-label="Quick folder view of {folderName(view.folder)}, outside the library" oncontextmenu={menu}>
    <header>
      <span class="title" title={view.folder}>Quick folder view: <b>{folderName(view.folder)}</b></span>
      <button class="icon" title="Open in {where}" aria-label="Open in {where}" onclick={reveal}>
        <!-- a folder with an arrow going out: opens outside the app -->
        <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M14.2 8.6v3.2c0 .7-.5 1.2-1.2 1.2H3c-.7 0-1.2-.5-1.2-1.2V4.2c0-.7.5-1.2 1.2-1.2h3l1.5 1.6H11" />
          <path d="M10.4 7.4 14.4 3.4M11.2 3.2h3.2v3.2" />
        </svg>
      </button>
      <button class="icon" title="Choose the folder, or hide the panel" aria-label="Menu" onclick={menu}>
        <svg viewBox="0 0 16 16" width="14" height="14" fill="currentColor" aria-hidden="true">
          <circle cx="3.5" cy="8" r="1.2" /><circle cx="8" cy="8" r="1.2" /><circle cx="12.5" cy="8" r="1.2" />
        </svg>
      </button>
    </header>
    {#if problem ?? view.error}
      <p class="note error">{problem ?? view.error}</p>
    {:else if !view.files.length}
      <p class="note">No PDF in this folder</p>
    {:else}
      <ul>
        {#each view.files as file (file.path)}
          <li
            class:in={file.matches.length > 0}
            title="{file.path}{file.matches.length ? '\n' + libraryNote(file.matches) : ''}\nDrag onto the table or onto a paper without a PDF; double-click to add"
            onpointerdown={(e) => press(e, file)}
            oncontextmenu={(e) => fileMenu(e, file)}
            ondblclick={() => onadd(file.path, null)}
          >
            <span class="name">{file.name}</span>
            <span class="meta">
              {formatSize(file.size)} · {formatDate(file.added)}
              {#if file.matches.length}<span class="check">✓ in the library</span>{/if}
            </span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

{#if pdfDrag.path}
  <!-- what is dragged, next to the pointer -->
  <div class="ghost" class:over={pdfDrag.target} style:left="{pdfDrag.x + 12}px" style:top="{pdfDrag.y + 12}px">
    {pdfDrag.name}
  </div>
{/if}

<style>
  .downloads {
    border-top: 1px dashed var(--border);
    background: color-mix(in srgb, var(--fg) 5%, transparent);
    -webkit-user-select: none;
    user-select: none;
  }
  header {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 5px 6px 2px 12px;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }
  .title b {
    color: var(--fg);
    font-weight: 600;
  }
  .icon {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: 5px;
    background: none;
    color: var(--muted);
    cursor: default;
  }
  .icon:hover {
    background: var(--line);
    color: var(--fg);
  }
  /* a long list scrolls within the panel: the rest of the sidebar keeps its room */
  ul {
    max-height: 26vh;
    margin: 0;
    padding: 0 0 4px;
    overflow-y: auto;
    list-style: none;
  }
  li {
    display: flex;
    flex-direction: column;
    margin: 0 6px;
    padding: 3px 6px;
    border-radius: 6px;
    cursor: default;
  }
  li:hover {
    background: var(--line);
  }
  .name {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .meta {
    color: var(--muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  /* in the library already: muted */
  li.in .name {
    color: var(--muted);
  }
  .check {
    margin-left: 4px;
    color: var(--accent);
  }
  .note {
    margin: 0;
    padding: 4px 12px 8px;
    color: var(--muted);
    font-size: 11.5px;
    word-break: break-word;
  }
  .note.error {
    color: var(--error);
  }
  .ghost {
    position: fixed;
    z-index: 20;
    max-width: 260px;
    padding: 3px 10px;
    overflow: hidden;
    border-radius: 6px;
    background: var(--accent);
    color: var(--on-accent);
    font-size: 12px;
    white-space: nowrap;
    text-overflow: ellipsis;
    pointer-events: none;
    opacity: 0.6;
  }
  /* over a place that takes it */
  .ghost.over {
    opacity: 1;
  }
</style>
