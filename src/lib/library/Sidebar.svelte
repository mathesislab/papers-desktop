<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { CheckMenuItem, Menu, MenuItem, Submenu } from "@tauri-apps/api/menu";
  import { foldTag, lastSegment, parentsOf, sameTag } from "$lib/tags";
  import { fileName, type View } from "./filters";
  import type { Row, TagCount } from "$lib/types";
  import { DROP_OPEN, rowDrag } from "./drag.svelte";

  let {
    libraries,
    bib,
    views,
    tags,
    view,
    filtered,
    pinned,
    open,
    tab,
    papers,
    active,
    dirty,
    onlibrary,
    onview,
    ontag,
    onfold,
    onrename,
    onpin,
    onlibrarytab,
    onpaper,
    onclose,
  }: {
    /** the .bib files, with their number of papers */
    libraries: { bib: string; count: number }[];
    /** the library shown by the library tab: a .bib file; none: all libraries */
    bib: string | null;
    /** the views, with their number of papers within the library */
    views: { view: View; name: string; count: number }[];
    /** the tags, with their counts within the library */
    tags: TagCount[];
    /** the view shown by the library tab */
    view: View;
    /** whether a tag is a filter of the library tab */
    filtered: (tag: string) => boolean;
    /** the tags pinned to the keys 1–9 (index = key − 1) */
    pinned: (string | null)[];
    /** the parent tags unfolded, folded as `foldTag` does */
    open: string[];
    /** the library tab, the first of the open ones: what it shows, and how many papers */
    tab: { label: string; count: number };
    /** the papers open, as tabs after it */
    papers: Row[];
    /** the paper in front; none: the library tab */
    active: string | null;
    /** the paper whose BibTeX has unsaved changes */
    dirty: string | null;
    /** a library chosen; none: all libraries */
    onlibrary: (bib: string | null) => void;
    onview: (view: View) => void;
    /** a tag clicked: to filter by it, or not any more */
    ontag: (tag: string) => void;
    /** a parent tag unfolded or folded by its triangle */
    onfold: (tag: string, open: boolean) => void;
    /** a tag renamed in its row */
    onrename: (tag: string, name: string) => void;
    /** a tag pinned to the key of a slot (index), or unpinned */
    onpin: (tag: string, slot: number | null) => void;
    /** the library tab chosen */
    onlibrarytab: () => void;
    onpaper: (id: string) => void;
    onclose: (id: string) => void;
  } = $props();

  /** the indent of a tag per level of the tree, px */
  const INDENT = 12;

  /** the key a tag is pinned to, 1–9 */
  const keyOf = (tag: string) => {
    const slot = pinned.findIndex((t) => t !== null && sameTag(t, tag));
    return slot < 0 ? null : slot + 1;
  };

  /** a row of the tree: a tag of the library or a parent of one */
  type Node = TagCount & { depth: number; children: Node[] };

  /** The tags as a tree, once per change of `tags`: each tag under its parent, in the order given. */
  let roots = $derived.by(() => {
    const nodes = tags.map((t): Node => ({ ...t, depth: 0, children: [] }));
    const byName = new Map(nodes.map((n) => [foldTag(n.tag), n]));
    const top: Node[] = [];
    for (const node of nodes) {
      const parent = byName.get(foldTag(parentsOf(node.tag).at(-1) ?? ""));
      (parent?.children ?? top).push(node);
    }
    const level = (list: Node[], depth: number) =>
      list.forEach((n) => {
        n.depth = depth;
        level(n.children, depth + 1);
      });
    level(top, 0);
    return top;
  });

  /** the parents of the tags the search filters: shown unfolded, so the filter is in view */
  let revealed = $derived(new Set(tags.filter((t) => filtered(t.tag)).flatMap((t) => parentsOf(t.tag).map(foldTag))));

  /** The rows shown: the tree in order, the tags under a folded parent left out. */
  let shown = $derived.by(() => {
    const unfolded = new Set(open);
    const rows: { node: Node; expanded: boolean; revealed: boolean }[] = [];
    const walk = (list: Node[]) => {
      for (const node of list) {
        const folded = foldTag(node.tag);
        const forced = revealed.has(folded);
        const expanded = forced || unfolded.has(folded);
        rows.push({ node, expanded, revealed: forced });
        if (expanded) walk(node.children);
      }
    };
    walk(roots);
    return rows;
  });

  /** the tag in its row being renamed, and the name so far */
  let editing = $state<string | null>(null);
  let draft = $state("");
  let field = $state<HTMLInputElement>();

  // the name is selected when the field comes
  $effect(() => {
    field?.focus();
    field?.select();
  });

  function edit(tag: string) {
    draft = tag;
    editing = tag;
  }

  /** Enter or leaving the field: an empty or unchanged name writes nothing. */
  function commit() {
    if (editing === null) return;
    const [tag, name] = [editing, draft.trim()];
    editing = null;
    if (name && name !== tag) onrename(tag, name);
  }

  function fieldKey(e: KeyboardEvent) {
    // the keys of the library (arrows, Enter, Escape) are not for the field
    e.stopPropagation();
    if (e.key === "Enter") {
      e.preventDefault();
      commit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      editing = null;
    }
  }

  /** Right-click on a tag: rename it, pin it to a key. */
  async function tagMenu(e: MouseEvent, tag: string) {
    e.preventDefault();
    const key = keyOf(tag);
    const slots = await Promise.all(
      Array.from({ length: 9 }, (_, slot) =>
        CheckMenuItem.new({
          text: pinned[slot] ? `${slot + 1}  ${pinned[slot]}` : `${slot + 1}`,
          checked: key === slot + 1,
          action: () => onpin(tag, slot),
        }),
      ),
    );
    const items = [
      await MenuItem.new({ text: "Rename…", action: () => edit(tag) }),
      await Submenu.new({ text: "Pin to Key", items: slots }),
    ];
    if (key) items.push(await MenuItem.new({ text: "Unpin", action: () => onpin(tag, null) }));
    const menu = await Menu.new({ items });
    await menu.popup();
  }
</script>

<div class="box">
<nav>
  {#if libraries.length > 1}
    <h4>Libraries</h4>
    <button class="item" class:selected={bib === null} onclick={() => onlibrary(null)}>
      <span class="name">All libraries</span><span class="count">{libraries.reduce((n, l) => n + l.count, 0)}</span>
    </button>
    {#each libraries as library}
      <button class="item" class:selected={bib === library.bib} title={library.bib} onclick={() => onlibrary(library.bib)}>
        <span class="name">{fileName(library.bib)}</span><span class="count">{library.count}</span>
      </button>
    {/each}
  {/if}

  <h4>Views</h4>
  {#each views as item}
    <button class="item" class:selected={view === item.view} onclick={() => onview(item.view)}>
      <span class="name">{item.name}</span><span class="count">{item.count}</span>
    </button>
  {/each}

  {#if tags.length}
    <h4>Tags</h4>
    {#each shown as { node: { tag, under, depth, children }, expanded, revealed } (tag)}
      {#if editing === tag}
        <!-- the full tag, so it can move: the field takes the whole row -->
        <div class="item">
          <input
            bind:this={field}
            bind:value={draft}
            aria-label="Rename the tag"
            spellcheck="false"
            autocomplete="off"
            onkeydown={fieldKey}
            onblur={commit}
          />
        </div>
      {:else}
        <div
          class="tag"
          class:selected={filtered(tag)}
          style:padding-left="{4 + depth * INDENT}px"
          role="presentation"
          oncontextmenu={(e) => tagMenu(e, tag)}
        >
          {#if children.length}
            <button
              class="twisty"
              class:open={expanded}
              disabled={revealed}
              aria-label={expanded ? `Fold ${tag}` : `Unfold ${tag}`}
              aria-expanded={expanded}
              onclick={() => onfold(tag, !expanded)}
            >
              <svg viewBox="0 0 8 8" width="8" height="8" aria-hidden="true">
                <path d="M2 1l4 3-4 3z" fill="currentColor" />
              </svg>
            </button>
          {:else}
            <span class="twisty"></span>
          {/if}
          <button class="label" title={tag} onclick={() => ontag(tag)}>
            <span class="name">{lastSegment(tag)}</span>
            {#if keyOf(tag)}<span class="key">{keyOf(tag)}</span>{/if}
            <span class="count">{under}</span>
          </button>
        </div>
      {/if}
    {/each}
  {/if}

  <!-- rows of the table dropped here open as tabs -->
  <div class="open" class:target={rowDrag.ids.length > 0} class:over={rowDrag.over} {...{ [DROP_OPEN]: "" }}>
  <h4>Open</h4>
  {#if rowDrag.ids.length}<div class="hint">Drop to open {rowDrag.ids.length === 1 ? "the paper" : `${rowDrag.ids.length} papers`}</div>{/if}
  <div class="paper" class:selected={active === null}>
    <button class="tab home" title={tab.label} onclick={onlibrarytab}>
      <span class="name">{tab.label}</span><span class="count">{tab.count}</span>
    </button>
  </div>
  {#each papers as paper (paper.id)}
    <div class="paper" class:selected={paper.id === active}>
      <button class="tab" title={paper.title} onclick={() => onpaper(paper.id)}>
        <span class="name">{paper.authors} {paper.year ?? ""}</span>
        <span class="title">{paper.title}</span>
      </button>
      {#if paper.id === dirty}<span class="dot" title="Unsaved changes"></span>{/if}
      <button class="close" aria-label="Close" onclick={() => onclose(paper.id)}>×</button>
    </div>
  {/each}
  </div>
</nav>

<footer>
  <button class="gear" title="Settings" aria-label="Settings" onclick={() => invoke("open_settings")}>
    <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  </button>
</footer>
</div>

<style>
  /* a drag of rows is on: the Open section says it takes them, the frame when the pointer is over it */
  .open {
    margin: 0 2px;
    padding: 0 0 4px;
    border: 1px dashed transparent;
    border-radius: 7px;
  }
  .open.target {
    border-color: var(--border);
  }
  .open.over {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }
  .hint {
    padding: 0 8px 4px;
    color: var(--accent);
    font-size: 11px;
  }
  /* the sidebar fills its pane: the gear stays at the bottom, over the list when it scrolls */
  .box {
    display: flex;
    flex-direction: column;
    min-height: 100%;
  }
  nav {
    flex: 1;
    padding: 6px 0 12px;
  }
  footer {
    position: sticky;
    bottom: 0;
    padding: 4px 8px;
    border-top: 1px solid var(--line);
    background: linear-gradient(var(--sidebar), var(--sidebar)), var(--bg);
  }
  .gear {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 5px;
    background: none;
    color: var(--muted);
    cursor: default;
  }
  .gear:hover {
    background: var(--line);
    color: var(--fg);
  }
  h4 {
    margin: 14px 16px 4px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    box-sizing: border-box;
    width: calc(100% - 12px);
    margin: 0 6px;
    padding: 4px 10px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: default;
  }
  .item.selected,
  .tag.selected {
    background: var(--line);
  }
  /* a tag of the tree: the triangle column, then the label as an item */
  .tag {
    display: flex;
    align-items: center;
    margin: 0 6px;
    border-radius: 6px;
  }
  .twisty {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    align-self: stretch;
    width: 16px;
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
  }
  .twisty svg {
    transition: transform 0.1s;
  }
  .twisty.open svg {
    transform: rotate(90deg);
  }
  /* the filter in it keeps it unfolded */
  .twisty:disabled {
    opacity: 0.45;
  }
  .label {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 10px 4px 0;
    border: none;
    background: none;
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: default;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .paper {
    display: flex;
    align-items: center;
    margin: 0 6px;
    border-radius: 6px;
  }
  .paper.selected {
    background: var(--line);
  }
  .tab {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: 4px 4px 4px 10px;
    border: none;
    background: none;
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: default;
  }
  /* the library tab: what it shows, and its count, in one line */
  .tab.home {
    flex-direction: row;
    align-items: center;
    gap: 8px;
    padding: 4px 10px;
  }
  .tab .title {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--muted);
    font-size: 11.5px;
  }
  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }
  .close {
    flex: none;
    width: 22px;
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
    font-size: 15px;
    visibility: hidden;
  }
  .paper:hover .close,
  .paper.selected .close {
    visibility: visible;
  }
  .item input {
    flex: 1;
    min-width: 0;
    height: 20px;
    padding: 0 4px;
    border: 1px solid var(--accent);
    border-radius: 4px;
    background: transparent;
    color: var(--fg);
    font: inherit;
    outline: none;
    -webkit-user-select: text;
    user-select: text;
  }
  .key {
    min-width: 14px;
    padding: 0 4px;
    border: 1px solid var(--line);
    border-radius: 4px;
    color: var(--muted);
    font-size: 10.5px;
    line-height: 14px;
    text-align: center;
  }
  .count {
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
</style>
