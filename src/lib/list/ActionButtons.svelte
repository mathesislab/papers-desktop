<script lang="ts" generics="N extends ActionName">
  // The buttons of the details of an item, in the Feeds and Search tabs: actions of the table (feeds/actions.ts),
  // each as a button says it (`buttonText`), its key after it, on the items given.

  import { buttonText, type Action, type ActionName, type Item } from "$lib/feeds/actions";

  let {
    actions,
    names,
    ids,
    items,
  }: {
    actions: Record<N, Action>;
    /** those shown, in their order */
    names: N[];
    /** what they act on */
    ids: string[];
    items: Item[];
  } = $props();
</script>

{#each names as name (name)}
  {@const action = actions[name]}
  <button class:primary={name === "add"} class:open={name === "open"} title={action.hint} disabled={!action.enabled(items)} onclick={() => action.run(ids)}>
    {buttonText(name, action, items)}{#if action.key}{" "}<kbd>{action.key.toUpperCase()}</kbd>{/if}
  </button>
{/each}
