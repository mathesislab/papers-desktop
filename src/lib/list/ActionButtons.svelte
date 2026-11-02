<script lang="ts" generics="N extends ActionName">
  // The buttons of the details of an item, in the Feeds and Search tabs: actions of the table (feeds/actions.ts),
  // each as a button says it (`buttonText`), its key after it, on the items given. A button with more (`menus`) has a
  // ▾ beside it, those in its menu (Add… ▾: Add without PDF…). An item in the library (`owned`): its Add is ✓ In
  // library, which shows it there, the adds in its menu.

  import { Menu, MenuItem } from "@tauri-apps/api/menu";
  import { buttonText, type Action, type ActionName, type Item } from "$lib/feeds/actions";
  import { menuItem } from "./ItemList.svelte";

  let {
    actions,
    names,
    menus = {},
    ids,
    items,
    owned,
  }: {
    actions: Record<N, Action>;
    /** those shown, in their order */
    names: N[];
    /** the actions in the menu of a button's ▾ */
    menus?: Partial<Record<N, N[]>>;
    /** what they act on */
    ids: string[];
    items: Item[];
    /** the item is in the library: shown there */
    owned?: () => void;
  } = $props();

  /** The menu of a button's ▾: the item shown in the library first, if it is there; the actions. */
  async function popup(more: N[], show?: () => void) {
    const shown = show ? [await MenuItem.new({ text: "Show in Library", action: show })] : [];
    const menu = await Menu.new({ items: [...shown, ...(await Promise.all(more.map((n) => menuItem(actions[n], ids, items))))] });
    await menu.popup();
  }
</script>

{#each names as name (name)}
  {@const action = actions[name]}
  {@const more = menus[name] ?? []}
  {#if name === "add" && owned}
    <span class="split">
      <button class="owned" title="In the library: show it there" onclick={owned}>✓ In library</button>
      <button class="owned caret" aria-label="More" title="Show it, or add it again" onclick={() => popup([name, ...more], owned)}>▾</button>
    </span>
  {:else}
    <span class:split={more.length}>
      <button class:primary={name === "add"} title={action.hint?.(items)} disabled={!action.enabled(items)} onclick={() => action.run(ids)}>
        {buttonText(name, action, items)}{#if action.key}{" "}<kbd>{action.key.toUpperCase()}</kbd>{/if}
      </button>
      {#if more.length}
        <button class="caret" class:primary={name === "add"} aria-label="More" onclick={() => popup(more)}>▾</button>
      {/if}
    </span>
  {/if}
{/each}

<style>
  .split {
    display: inline-flex;
  }
  .split button:first-child {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }
  .split .caret {
    padding-right: 6px;
    padding-left: 6px;
    border-left-color: color-mix(in srgb, currentColor 30%, transparent);
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
  }
  /* in the library: the green of its ✓, quietly */
  .owned {
    border-color: color-mix(in srgb, #2f9e6e 45%, transparent);
    background: color-mix(in srgb, #2f9e6e 10%, transparent);
    color: #2f9e6e;
  }
</style>
