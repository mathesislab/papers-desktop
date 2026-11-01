<script lang="ts">
  // Simple | Table, above the list of each tab: how the list shows (`Look`), which the window's layout keeps. The
  // one on is in the tab's colour (`--tint`; the library's, its accent).

  import type { Look } from "./columns";

  let { look = $bindable() }: { look: Look } = $props();

  /** the looks, as the switch says them */
  const LOOKS: { look: Look; name: string; title: string }[] = [
    { look: "simple", name: "Simple", title: "Each in three lines: its journal or source, its title, its authors" },
    { look: "table", name: "Table", title: "The columns, under their headers: to move, resize, show or hide them" },
  ];
</script>

<span class="looks" role="radiogroup" aria-label="How the list shows">
  {#each LOOKS as l (l.look)}
    <button role="radio" aria-checked={look === l.look} class:on={look === l.look} title={l.title} onclick={() => (look = l.look)}>
      {l.name}
    </button>
  {/each}
</span>

<style>
  /* two halves, the one on tinted */
  .looks {
    flex: none;
    display: inline-flex;
    margin-left: 4px;
    border: 1px solid var(--border);
    border-radius: 5px;
    overflow: hidden;
  }
  .looks button {
    padding: 0 7px;
    border: none;
    background: none;
    color: var(--muted);
    font: inherit;
    font-size: 11px;
    line-height: 17px;
  }
  .looks button + button {
    border-left: 1px solid var(--border);
  }
  .looks button.on {
    background: color-mix(in srgb, var(--tint, var(--accent)) 14%, transparent);
    color: var(--fg);
  }
</style>
