<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { label } from "$lib/keys";
  import type { Reader } from "./reader.svelte";

  /** The controls of a PDF, in the toolbar of the window. */
  let { reader, onclose }: { reader: Reader; onclose: () => void } = $props();

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      reader.search(true, e.shiftKey);
    } else if (e.key === "Escape") {
      e.preventDefault();
      reader.closeFind();
    }
  }
</script>

{#if reader.outline.length}
  <button class="icon" class:on={reader.showOutline} title="Outline" aria-label="Outline" onclick={() => (reader.showOutline = !reader.showOutline)}>
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"
      ><path d="M2 3.5h12M5 8h9M5 12.5h9" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" /></svg
    >
  </button>
{/if}
<input
  bind:this={reader.findInput}
  bind:value={reader.find}
  type="search"
  placeholder="Find in the PDF"
  spellcheck="false"
  oninput={() => reader.search(false)}
  {onkeydown}
/>
<span class="pages" title={reader.count ? `Page ${reader.current} of ${reader.count}` : ""}
  >{reader.count ? `${reader.current} / ${reader.count}` : ""}</span
>
<span class="zoom">
  <button title="Zoom out" onclick={() => reader.zoom(-1)}>−</button><button title="Fit the width" onclick={() => reader.zoom(0)}
    >Fit</button
  ><button title="Zoom in" onclick={() => reader.zoom(1)}>+</button>
</span>
<button class="icon" title="Open in the default PDF app" aria-label="Open in the default PDF app" onclick={() => invoke("open", { id: reader.id })}>↗</button>
<button class="icon close" title="Close the PDF ({label({ key: 'w', mod: true })})" aria-label="Close the PDF" onclick={onclose}>✕</button>

<style>
  button {
    padding: 3px 9px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
  }
  /* a mark alone, its words on hover; on one line, at its width: the find field gives the room */
  button {
    flex: none;
    white-space: nowrap;
  }
  /* − Fit +, one group */
  .zoom {
    flex: none;
    display: flex;
  }
  .zoom button {
    border-radius: 0;
  }
  .zoom button:first-child {
    border-radius: 6px 0 0 6px;
  }
  .zoom button:last-child {
    border-radius: 0 6px 6px 0;
  }
  .zoom button + button {
    border-left: none;
  }
  button.icon {
    min-width: 28px;
    padding: 3px 7px;
  }
  button.icon svg {
    display: block;
  }
  button.on {
    background: var(--line);
  }
  /* at the right end, lightly in the accent: the way out of the PDF */
  button.close {
    margin-left: auto;
    border-color: color-mix(in srgb, var(--accent) 35%, transparent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent);
    font-weight: 500;
  }
  button.close:hover {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
  }
  input {
    flex: 1 1 200px;
    min-width: 70px;
    max-width: 360px;
    height: 28px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: transparent;
    color: var(--fg);
    font: inherit;
    outline: none;
  }
  input:focus {
    border-color: var(--accent);
  }
  .pages {
    flex: none;
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  /* a narrow bar (the window, the sidebar): the page count goes, the buttons stay (header.reading is the container) */
  @container (max-width: 1080px) {
    .pages {
      display: none;
    }
  }
</style>
