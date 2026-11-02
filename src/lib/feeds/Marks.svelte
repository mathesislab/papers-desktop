<script lang="ts">
  // The status of an item as symbols (`marks`): its version, a half circle (◐ a preprint, ◑ published, ● both) or, a
  // post, its kind (a newspaper: news; ✎ general); ✓ in the library; a newspaper: news that covered it; ⏰ its reminder. What each says is
  // its hover. In a table's column, each in its slot, so that they line up from row to row (`slots`); beside a simple
  // row's source, those there (`inline`); in the details, each with its words, an entry of the library to show
  // (`words`).

  import { MARK_SLOTS, type Mark } from "./items";

  let {
    marks,
    look,
    onentry,
  }: {
    marks: Mark[];
    look: "slots" | "inline" | "words";
    /** in words: an entry of the library shown */
    onentry?: (id: string) => void;
  } = $props();

  let bySlot = $derived(new Map(marks.map((m) => [m.slot, m])));
  let title = $derived(marks.map((m) => m.text).join(" · "));
</script>

{#snippet symbol(m: Mark)}
  {#if m.symbol === "preprint" || m.symbol === "published" || m.symbol === "both"}
    <svg class="stage" viewBox="0 0 12 12" width="11" height="11" aria-hidden="true">
      {#if m.symbol === "both"}
        <circle cx="6" cy="6" r="5" class="fill" />
      {:else}
        <path d={m.symbol === "preprint" ? "M6 1a5 5 0 0 0 0 10z" : "M6 1a5 5 0 0 1 0 10z"} class="fill" />
      {/if}
      <circle cx="6" cy="6" r="5" class="ring" />
    </svg>
  {:else if m.symbol === "news" || m.symbol === "covered"}
    <svg class="paper" viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
      <rect x="1.5" y="2" width="9" height="8" rx="1" />
      <path d="M3.5 4.5h5M3.5 6.5h5M3.5 8h3" />
    </svg>
  {:else}
    <span class="sym {m.symbol}" aria-hidden="true">{{ blog: "✎", library: "✓", due: "⏰", snoozed: "⏰" }[m.symbol]}</span>
  {/if}
{/snippet}

{#if look === "words"}
  <span class="marks words">
    {#each marks as m, i (m.slot)}
      {#if i}<span class="sep">·</span>{/if}
      <span class="mark">
        {@render symbol(m)}
        {#if m.slot === "library"}
          {#each m.entries as e, j (e.id)}
            {#if j}{", "}{/if}{e.bib ? `in ${e.bib}` : "in library"}
            {#if onentry}<button class="link" onclick={() => onentry(e.id)}>show</button>{/if}
          {/each}
        {:else}
          {m.text}
        {/if}
      </span>
    {/each}
  </span>
{:else if look === "slots"}
  <span class="marks slots" {title}>
    {#each MARK_SLOTS as slot (slot)}
      {@const m = bySlot.get(slot)}
      <span class="slot">{#if m}{@render symbol(m)}{/if}</span>
    {/each}
  </span>
{:else}
  <span class="marks inline" {title}>
    {#each marks as m (m.slot)}{@render symbol(m)}{/each}
  </span>
{/if}

<style>
  .marks {
    display: inline-flex;
    align-items: center;
    color: var(--muted);
    font-size: 11px;
    line-height: 14px;
  }
  .slots .slot {
    flex: none;
    width: 13px;
    display: inline-flex;
    justify-content: center;
  }
  .inline {
    flex: none;
    gap: 4px;
  }
  .words {
    flex-wrap: wrap;
    gap: 4px 6px;
    font-size: inherit;
    line-height: inherit;
  }
  .words .mark {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  /* the version: a ring, its stages filled lightly */
  .stage {
    display: block;
    flex: none;
  }
  /* the ring quieter than the fill */
  .stage .ring {
    fill: none;
    stroke: currentColor;
    stroke-opacity: 0.55;
    stroke-width: 1;
  }
  .stage .fill {
    fill: currentColor;
    fill-opacity: 0.45;
  }
  /* news: a newspaper, drawn as the ring is */
  .paper {
    display: block;
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 0.9;
  }
  .sym.library {
    color: #2f9e6e;
  }
  .sym.snoozed {
    opacity: 0.5;
  }
  .link {
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }
  :global(.row.selected) .marks,
  :global(.row.selected) .sym.library {
    color: inherit;
  }
</style>
