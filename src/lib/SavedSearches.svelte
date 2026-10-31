<script lang="ts">
  // The Saved section at the top of a tab's sidebar: the saved searches with their counts; a click shows one in
  // place of the filters chosen, the one shown is selected. Right-click: rename it (in its row), update it to the
  // filters shown, delete it.

  import { Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";

  let {
    searches,
    shown,
    refusal,
    onapply,
    onrename,
    onupdate,
    ondelete,
  }: {
    /** in their order, each with the number of what it shows */
    searches: { name: string; count: number }[];
    /** the name of the saved search the tab shows; none */
    shown: string | null;
    /** why a search cannot be renamed so (its own name aside); none: it can */
    refusal: (name: string, own: string) => string | null;
    onapply: (name: string) => void;
    onrename: (name: string, to: string) => void;
    /** the filters the tab shows saved in its place */
    onupdate: (name: string) => void;
    ondelete: (name: string) => void;
  } = $props();

  /** the search being renamed in its row, and the name so far */
  let editing = $state<string | null>(null);
  let draft = $state("");
  let field = $state<HTMLInputElement>();
  let refused = $derived(editing === null ? null : refusal(draft, editing));

  // the name is selected when the field comes
  $effect(() => {
    field?.focus();
    field?.select();
  });

  function edit(name: string) {
    draft = name;
    editing = name;
  }

  /** Enter or leaving the field: an unchanged name, or one refused, renames nothing. */
  function commit() {
    if (editing === null) return;
    const [name, to] = [editing, draft.trim()];
    editing = null;
    if (to !== name && refusal(to, name) === null) onrename(name, to);
  }

  function fieldKey(e: KeyboardEvent) {
    // the keys of the tab are not for the field
    e.stopPropagation();
    if (e.key === "Enter") {
      e.preventDefault();
      commit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      editing = null;
    }
  }

  async function menu(e: MouseEvent, name: string) {
    e.preventDefault();
    const items = [
      await MenuItem.new({ text: "Rename…", action: () => edit(name) }),
      await MenuItem.new({ text: "Update to the Filters Shown", enabled: shown !== name, action: () => onupdate(name) }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await MenuItem.new({ text: "Delete", action: () => ondelete(name) }),
    ];
    await (await Menu.new({ items })).popup();
  }
</script>

{#if searches.length}
  <h4>Saved</h4>
  {#each searches as search (search.name)}
    {#if editing === search.name}
      <div class="item" title={refused ?? undefined}>
        <input
          bind:this={field}
          bind:value={draft}
          class:refused={refused !== null && draft.trim() !== search.name}
          aria-label="Rename the saved search"
          spellcheck="false"
          autocomplete="off"
          onkeydown={fieldKey}
          onblur={commit}
        />
      </div>
    {:else}
      <button
        class="item"
        class:selected={shown === search.name}
        title={search.name}
        onclick={() => onapply(search.name)}
        oncontextmenu={(e) => menu(e, search.name)}
      >
        <span class="mark">★</span>
        <span class="name">{search.name}</span><span class="count">{search.count}</span>
      </button>
    {/if}
  {/each}
{/if}

<style>
  /* as the rows of the sidebars */
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
  .item.selected {
    background: var(--line);
  }
  .mark {
    flex: none;
    width: 14px;
    color: #d9822b;
    font-size: 11px;
    text-align: center;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .count {
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  input {
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
  input.refused {
    border-color: var(--error);
  }
</style>
