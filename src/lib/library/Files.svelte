<script lang="ts">
  // The files of a paper, in their order: the first is its PDF (Open PDF, Space). A file that is gone, in red, to
  // locate (a file in its place). Each dragged by its grip to another place; its ⋯: Open, Reveal in Finder, Make
  // first, Remove from paper (the file kept), Move to Trash (undone with the library). Add file… adds files after
  // the others; a PDF dropped on the section goes to the Add sheet, for this paper (`data-drop-pdf`).

  import { invoke } from "@tauri-apps/api/core";
  import { Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
  import { ask, open } from "@tauri-apps/plugin-dialog";
  import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
  import { mac } from "$lib/keys";
  import type { Item } from "$lib/types";

  let {
    item,
    onset,
    onattach,
  }: {
    item: Item;
    /** the files set: those kept in their order, those of `del` deleted */
    onset: (files: string[], del: string[]) => unknown;
    /** files added, after the others or in place of `replace` */
    onattach: (files: string[], replace: string | null) => unknown;
  } = $props();

  /** mirrors FileInfo of src-tauri */
  type Info = { path: string; exists: boolean; size: number | null };

  /** the files as they are: there or not, their sizes */
  let infos = $state<Info[]>([]);
  // again when the paper or its files change
  $effect(() => {
    const { id, files } = item;
    let live = true;
    invoke<Info[]>("files_of", { id })
      .then((found) => live && (infos = found))
      .catch(() => live && (infos = files.map((path) => ({ path, exists: true, size: null }))));
    return () => {
      live = false;
    };
  });

  const nameOf = (path: string) => path.split(/[\\/]/).pop() ?? path;
  function sizeText(size: number | null): string {
    if (size === null) return "";
    if (size < 1024 * 1024) return `${Math.max(1, Math.round(size / 1024))} KB`;
    return `${(size / 1024 / 1024).toFixed(1)} MB`;
  }

  let paths = $derived(infos.map((i) => i.path));
  const without = (path: string) => paths.filter((p) => p !== path);

  /** The menu of a file: what it is done with. */
  async function menu(info: Info, at: number) {
    const name = nameOf(info.path);
    const item = (text: string, action: () => unknown, enabled = true) => MenuItem.new({ text, action, enabled });
    const items = await Promise.all([
      ...(info.exists
        ? [item("Open", () => openPath(info.path)), item(mac ? "Reveal in Finder" : "Show in Folder", () => revealItemInDir(info.path))]
        : [item("Locate…", () => locate(info.path))]),
      item("Make First", () => onset([info.path, ...without(info.path)], []), at > 0),
      PredefinedMenuItem.new({ item: "Separator" }),
      item("Remove from Paper", () => onset(without(info.path), [])),
      item("Move to Trash…", () => trash(info.path, name), info.exists),
    ]);
    await (await Menu.new({ items })).popup();
  }

  async function trash(path: string, name: string) {
    const yes = await ask(`Move ${name} to the Trash? Undo brings it back.`, {
      title: "Move to Trash",
      kind: "warning",
      okLabel: "Move to Trash",
      cancelLabel: "Cancel",
    });
    if (yes) onset(without(path), [path]);
  }

  /** A file in place of one gone. */
  async function locate(gone: string) {
    const picked = await open({ title: `In place of ${nameOf(gone)}` });
    if (typeof picked === "string") onattach([picked], gone);
  }

  /** Files after the others. */
  async function addFiles() {
    const picked = await open({ multiple: true, title: "Add files to this paper" });
    const files = Array.isArray(picked) ? picked : picked ? [picked] : [];
    if (files.length) onattach(files, null);
  }

  // a file dragged by its grip: where it would go
  let dragged = $state<number | null>(null);
  let over = $state<number | null>(null);
  let list = $state<HTMLElement>();

  function grab(e: PointerEvent, at: number) {
    e.preventDefault();
    dragged = at;
    over = at;
    const move = (ev: PointerEvent) => {
      const rows = [...(list?.querySelectorAll<HTMLElement>(".file") ?? [])];
      const below = rows.findIndex((row) => ev.clientY < row.getBoundingClientRect().top + row.offsetHeight / 2);
      over = below < 0 ? rows.length - 1 : below;
    };
    const drop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", drop);
      const [from, to] = [dragged, over];
      dragged = over = null;
      if (from === null || to === null || from === to) return;
      const order = [...paths];
      const [moved] = order.splice(from, 1);
      order.splice(to, 0, moved);
      onset(order, []);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", drop);
  }
</script>

<section class="files" data-drop-pdf={item.id}>
  <h4>Files</h4>
  {#if infos.length}
    <ul bind:this={list}>
      {#each infos as info, i (info.path)}
        <li class="file" class:missing={!info.exists} class:dragged={dragged === i} class:over={over === i && dragged !== i}>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span class="grip" title="Drag to another place" onpointerdown={(e) => grab(e, i)}>⋮⋮</span>
          <button
            class="name"
            title={info.exists ? `${info.path}\nOpen` : `${info.path}\nNot there any more`}
            disabled={!info.exists}
            ondblclick={() => openPath(info.path)}
            onclick={() => info.exists && openPath(info.path)}>{nameOf(info.path)}</button
          >
          {#if !info.exists}
            <span class="gone">missing</span>
            <button class="small" onclick={() => locate(info.path)}>Locate…</button>
          {:else}
            {#if i === 0}<span class="first" title="Open PDF and Space open it">opens</span>{/if}
            <span class="size">{sizeText(info.size)}</span>
          {/if}
          <button class="more" aria-label="What to do with {nameOf(info.path)}" onclick={() => menu(info, i)}>⋯</button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="none">No file</p>
  {/if}
  <div class="add">
    <button class="small" onclick={addFiles}>Add file…</button>
    <span class="hint">or drop a PDF here</span>
  </div>
</section>

<style>
  .files {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  h4 {
    margin: 0;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 4px;
    border-top: 2px solid transparent;
    border-radius: 4px;
    font-size: 12px;
  }
  .file:hover {
    background: var(--line);
  }
  .file.dragged {
    opacity: 0.4;
  }
  /* where the file dragged goes: before this one */
  .file.over {
    border-top-color: var(--accent);
  }
  .grip {
    flex: none;
    color: var(--muted);
    font-size: 10px;
    letter-spacing: -2px;
    cursor: grab;
    user-select: none;
  }
  .name {
    flex: 1;
    min-width: 0;
    padding: 0;
    overflow: hidden;
    border: none;
    background: none;
    color: var(--fg);
    font: inherit;
    text-align: left;
    white-space: nowrap;
    text-overflow: ellipsis;
    cursor: pointer;
  }
  .missing .name {
    color: var(--error);
    cursor: default;
  }
  .gone,
  .size,
  .hint,
  .none {
    flex: none;
    color: var(--muted);
    font-size: 11px;
  }
  .none {
    margin: 0;
  }
  .first {
    flex: none;
    padding: 0 5px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent);
    font-size: 10px;
  }
  .more {
    flex: none;
    padding: 0 4px;
    border: none;
    background: none;
    color: var(--muted);
    font: inherit;
    cursor: pointer;
  }
  .more:hover {
    color: var(--fg);
  }
  .small {
    padding: 1px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 11px;
  }
  .add {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 2px;
  }
</style>
