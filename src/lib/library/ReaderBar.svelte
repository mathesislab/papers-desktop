<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { PdfReader } from "./reader.svelte";

  /** The controls of a PDF, in the toolbar of the window. */
  let { reader }: { reader: PdfReader } = $props();

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
  <button class:on={reader.showOutline} onclick={() => (reader.showOutline = !reader.showOutline)}>Outline</button>
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
<span class="pages">{reader.count ? `Page ${reader.current} of ${reader.count}` : ""}</span>
<button title="Zoom out" onclick={() => reader.zoom(-1)}>−</button>
<button title="Fit the width" onclick={() => reader.zoom(0)}>Fit</button>
<button title="Zoom in" onclick={() => reader.zoom(1)}>+</button>
<button title="Open in the default PDF app" onclick={() => invoke("open", { id: reader.id })}>Open ↗</button>

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
  button.on {
    background: var(--line);
  }
  input {
    width: min(360px, 40%);
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
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
</style>
