<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import type { Entry, Reader } from "./reader.svelte";

  /** The pages of a PDF, its outline in a strip at their left when asked. */
  let { reader }: { reader: Reader } = $props();

  let area = $state<HTMLDivElement>();

  onMount(() => reader.mount(area!));

  /** The links of a PDF to the web open in the browser, not in the window. */
  function onclick(e: MouseEvent) {
    const link = (e.target as HTMLElement).closest("a");
    if (link?.href && /^(https?|mailto):/.test(link.href)) {
      e.preventDefault();
      e.stopPropagation();
      openUrl(link.href);
    }
  }
</script>

{#snippet entries(list: Entry[])}
  <ul>
    {#each list as entry}
      <li>
        <button onclick={() => reader.go(entry)}>{entry.title}</button>
        {#if entry.items.length}{@render entries(entry.items)}{/if}
      </li>
    {/each}
  </ul>
{/snippet}

<div class="reader">
  {#if reader.showOutline}
    <nav class="outline">{@render entries(reader.outline)}</nav>
  {/if}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="area" bind:this={area} onclickcapture={onclick}>
    {#if reader.error}<p class="error">{reader.error}</p>{/if}
  </div>
</div>

<style>
  .reader {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .area {
    flex: 1;
    min-width: 0;
    position: relative;
    background: var(--line);
  }
  /* made by the pdf.js reader */
  .area :global(.pdfjs-container) {
    position: absolute;
    inset: 0;
    overflow: auto;
    -webkit-user-select: text;
    user-select: text;
  }
  .outline {
    flex: none;
    width: 240px;
    overflow-y: auto;
    padding: 8px 0;
    border-right: 1px solid var(--line);
  }
  .outline ul {
    margin: 0;
    padding: 0 0 0 10px;
    list-style: none;
  }
  .outline button {
    display: block;
    width: 100%;
    padding: 3px 8px;
    border: none;
    border-radius: 4px;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
    text-align: left;
  }
  .outline button:hover {
    background: var(--line);
  }
  .error {
    position: absolute;
    inset: 0;
    z-index: 1;
    margin: auto;
    height: fit-content;
    color: var(--error);
    text-align: center;
  }
</style>
