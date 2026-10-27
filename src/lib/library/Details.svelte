<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { mac } from "$lib/keys";
  import Preview from "$lib/Preview.svelte";
  import type { Item } from "$lib/types";

  let { item, onopen }: { item: Item; onopen: () => void } = $props();

  let copied = $state(false);

  $effect(() => {
    item.id;
    copied = false;
  });

  async function copyKey() {
    await invoke("copy", { id: item.id, format: "pandoc" });
    copied = true;
  }
</script>

<div class="details">
  <div class="actions">
    {#if item.files.length || item.doi || item.url}
      <button class="primary" onclick={onopen}>{item.files.length ? "Open PDF" : "Open DOI page"}</button>
    {/if}
    {#if item.files.length}
      <button onclick={() => invoke("reveal", { id: item.id })}>{mac ? "Reveal in Finder" : "Show in folder"}</button>
    {/if}
    <button onclick={copyKey}>{copied ? "Copied" : "Copy [@key]"}</button>
  </div>
  <Preview {item} marked={item.authors.map(() => false)} {onopen} />
</div>

<style>
  .details {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 12px 16px 0;
  }
  button {
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
  }
  button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
</style>
