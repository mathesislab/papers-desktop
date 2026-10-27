<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import "pdfjs-dist/web/pdf_viewer.css";
  import { onMount } from "svelte";
  import type { Entry, PdfReader } from "./reader.svelte";

  /** The pages of a PDF, and its outline over them when asked. */
  let { reader }: { reader: PdfReader } = $props();

  let container = $state<HTMLDivElement>();
  let pages = $state<HTMLDivElement>();

  onMount(() => reader.mount(container!, pages!));

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

<div class="area">
  {#if reader.showOutline}
    <nav class="outline">{@render entries(reader.outline)}</nav>
  {/if}
  {#if reader.error}
    <p class="error">{reader.error}</p>
  {/if}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="container" bind:this={container} onclickcapture={onclick}>
    <div class="pdfViewer" bind:this={pages}></div>
  </div>
</div>

<style>
  .area {
    flex: 1;
    position: relative;
    background: var(--line);
  }
  .container {
    position: absolute;
    inset: 0;
    overflow: auto;
    -webkit-user-select: text;
    user-select: text;
  }
  .outline {
    position: absolute;
    top: 8px;
    left: 8px;
    bottom: 8px;
    z-index: 2;
    width: 280px;
    overflow-y: auto;
    padding: 6px 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--menu-bg);
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
