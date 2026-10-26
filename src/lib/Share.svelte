<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { Item } from "$lib/types";

  let {
    item,
    citation,
  }: {
    item: Item;
    /** the formatted citation, as HTML, once made */
    citation: string | null;
  } = $props();

  let open = $state(false);
  let copied = $state(false);
  let root: HTMLElement;

  let link = $derived(item.doi ? `https://doi.org/${item.doi}` : item.url);
  let citationText = $derived(
    citation &&
      (new DOMParser().parseFromString(citation, "text/html").body.textContent ?? "")
        .replace(/\s+/g, " ")
        .trim(),
  );
  /** what each choice copies; nothing when the paper does not have it */
  let choices = $derived([
    { label: "Snippet (title + link)", text: link ? `${item.title}\n${link}` : item.title },
    { label: "Link", text: link },
    { label: "DOI", text: item.doi },
    { label: "Citation", text: citationText },
  ]);

  // another paper: the menu closes
  $effect(() => {
    void item.id;
    open = false;
  });

  async function share(text: string) {
    open = false;
    await invoke("copy_text", { text });
    copied = true;
    setTimeout(() => (copied = false), 1200);
  }

  function outside(e: MouseEvent) {
    if (open && !root.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onclick={outside} />

<!-- the search field keeps the focus -->
<span class="share" bind:this={root}>
  <button
    class="toggle"
    aria-expanded={open}
    onmousedown={(e) => e.preventDefault()}
    onclick={() => (open = !open)}>{copied ? "Copied ✓" : "Share ▾"}</button
  >
  {#if open}
    <div class="menu" role="menu">
      {#each choices as choice}
        <button
          role="menuitem"
          disabled={!choice.text}
          onmousedown={(e) => e.preventDefault()}
          onclick={() => choice.text && share(choice.text)}>{choice.label}</button
        >
      {/each}
    </div>
  {/if}
</span>

<style>
  .share {
    position: relative;
  }
  button {
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }
  .toggle {
    padding: 0 6px;
    border: 1px solid var(--line);
    border-radius: 4px;
    background: none;
    color: var(--fg);
  }
  .toggle:hover {
    border-color: var(--muted);
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 1;
    display: flex;
    flex-direction: column;
    min-width: 170px;
    padding: 4px;
    background: var(--menu-bg);
    border: 1px solid var(--border);
    border-radius: 7px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.22);
  }
  .menu button {
    padding: 5px 8px;
    border: none;
    border-radius: 5px;
    background: none;
    color: var(--fg);
    font-size: 12px;
    text-align: left;
  }
  .menu button:hover:not(:disabled) {
    background: var(--accent);
    color: var(--on-accent);
  }
  .menu button:disabled {
    color: var(--muted);
    cursor: default;
  }
</style>
