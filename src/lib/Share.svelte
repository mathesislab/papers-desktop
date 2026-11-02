<script lang="ts">
  // Share: a click copies the snippet (the title and its link, as plain text); ▾ the other ways (the link, the DOI,
  // the citation, as text or HTML).
  import { toCopy } from "$lib/cite";
  import { copy, type Copy } from "$lib/clipboard";
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
  let cited = $derived(citation ? toCopy(citation) : null);
  /** what each choice copies; nothing when the paper does not have it */
  let choices = $derived<{ label: string; copy: Copy | null }[]>([
    // plain text: an app that takes HTML would make a link of the title (in Markdown, `[title](link)`)
    { label: "Snippet (title and link)", copy: { text: link ? `${item.title}\n${link}` : item.title } },
    { label: "Link", copy: link ? { text: link } : null },
    { label: "DOI", copy: item.doi ? { text: item.doi } : null },
    { label: "Citation", copy: cited },
    { label: "Citation (HTML)", copy: cited && { text: cited.html } },
  ]);

  // another paper: the menu closes
  $effect(() => {
    void item.id;
    open = false;
  });

  async function share(choice: Copy) {
    open = false;
    await copy(item.id, choice);
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
  <button class="toggle main" title="Copy the title and its link" onmousedown={(e) => e.preventDefault()} onclick={() => share(choices[0].copy!)}
    >{copied ? "Copied ✓" : "Share"}</button
  ><button
    class="toggle caret"
    aria-label="Other ways to share"
    aria-expanded={open}
    onmousedown={(e) => e.preventDefault()}
    onclick={() => (open = !open)}>▾</button
  >
  {#if open}
    <div class="menu" role="menu">
      {#each choices as choice}
        <button
          role="menuitem"
          disabled={!choice.copy}
          onmousedown={(e) => e.preventDefault()}
          onclick={() => choice.copy && share(choice.copy)}>{choice.label}</button
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
  .main {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }
  .caret {
    margin-left: -1px;
    padding: 0 4px;
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
  }
  .menu {
    position: absolute;
    left: 0;
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
