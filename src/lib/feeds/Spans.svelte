<script lang="ts">
  // A text of the feeds marked where the query or an interest matched: highlighted, as a reader marks a page.

  import type { Span } from "$lib/types";

  let { spans }: { spans: Span[] } = $props();
</script>

{#each spans as span}{#if span.mark}<mark>{span.text}</mark>{:else}{span.text}{/if}{/each}

<style>
  mark {
    padding: 0 1px;
    border-radius: 2px;
    background: color-mix(in srgb, #facc15 45%, transparent);
    color: inherit;
  }
</style>
