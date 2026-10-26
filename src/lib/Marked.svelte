<script lang="ts">
  import type { Span } from "$lib/types";

  let { spans }: { spans: Span[] } = $props();
</script>

{#each spans as span}{#if span.mark}<b>{span.text}</b>{:else}{span.text}{/if}{/each}

<style>
  b {
    font-weight: 700;
    color: var(--mark, inherit);
  }
</style>
