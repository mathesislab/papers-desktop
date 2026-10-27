<script lang="ts" module>
  /** what the table lists: all papers, a view of them, or one .bib file */
  export type View = "all" | "recent" | "nopdf" | "broken" | "duplicates" | { bib: string };

  /** whether the library shows view `b`: not while a paper is in front */
  export const sameView = (a: View | null, b: View) =>
    a !== null && (typeof a === "string" || typeof b === "string" ? a === b : a.bib === b.bib);
</script>

<script lang="ts">
  import { CheckMenuItem, Menu, MenuItem, Submenu } from "@tauri-apps/api/menu";
  import { sameTag } from "$lib/tags";
  import type { Row, TagCount } from "$lib/types";

  let {
    libraries,
    views,
    tags,
    view,
    filtered,
    pinned,
    papers,
    active,
    onview,
    ontag,
    onrename,
    onpin,
    onpaper,
    onclose,
  }: {
    /** the .bib files, with their number of papers */
    libraries: { bib: string; count: number }[];
    views: { view: View; name: string; count: number }[];
    tags: TagCount[];
    /** the view of the library; none while a paper is in front */
    view: View | null;
    /** whether the search filters a tag */
    filtered: (tag: string) => boolean;
    /** the tags pinned to the keys 1–9 (index = key − 1) */
    pinned: (string | null)[];
    /** the papers open, as tabs */
    papers: Row[];
    /** the paper in front */
    active: string | null;
    onview: (view: View) => void;
    ontag: (tag: string) => void;
    /** a tag renamed in its row */
    onrename: (tag: string, name: string) => void;
    /** a tag pinned to the key of a slot (index), or unpinned */
    onpin: (tag: string, slot: number | null) => void;
    onpaper: (id: string) => void;
    onclose: (id: string) => void;
  } = $props();

  const fileName = (path: string) => path.split(/[\\/]/).pop();

  /** the key a tag is pinned to, 1–9 */
  const keyOf = (tag: string) => {
    const slot = pinned.findIndex((t) => t !== null && sameTag(t, tag));
    return slot < 0 ? null : slot + 1;
  };

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

<nav>
  {#if libraries.length > 1}
    <h4>Libraries</h4>
    {#each libraries as { bib, count }}
      <button class="item" class:selected={sameView(view, { bib })} title={bib} onclick={() => onview({ bib })}>
        <span class="name">{fileName(bib)}</span><span class="count">{count}</span>
      </button>
    {/each}
  {/if}

  <h4>Library</h4>
  {#each views as item}
    <button class="item" class:selected={sameView(view, item.view)} onclick={() => onview(item.view)}>
      <span class="name">{item.name}</span><span class="count">{item.count}</span>
    </button>
  {/each}

  {#if tags.length}
    <h4>Tags</h4>
    {#each tags as { tag, count } (tag)}
      {#if editing === tag}
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
        <button
          class="item"
          class:selected={filtered(tag)}
          onclick={() => ontag(tag)}
          oncontextmenu={(e) => tagMenu(e, tag)}
        >
          <span class="name">{tag}</span>
          {#if keyOf(tag)}<span class="key">{keyOf(tag)}</span>{/if}
          <span class="count">{count}</span>
        </button>
      {/if}
    {/each}
  {/if}

  {#if papers.length}
    <h4>Open papers</h4>
    {#each papers as paper (paper.id)}
      <div class="paper" class:selected={paper.id === active}>
        <button class="tab" title={paper.title} onclick={() => onpaper(paper.id)}>
          <span class="name">{paper.authors} {paper.year ?? ""}</span>
          <span class="title">{paper.title}</span>
        </button>
        <button class="close" aria-label="Close" onclick={() => onclose(paper.id)}>×</button>
      </div>
    {/each}
  {/if}
</nav>

<style>
  nav {
    padding: 6px 0 12px;
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
  .item.selected {
    background: var(--line);
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
  .tab .title {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--muted);
    font-size: 11.5px;
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
