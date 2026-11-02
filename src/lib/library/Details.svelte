<script lang="ts">
  // The details of the paper in focus in the Library tab, as those of the Feeds and Search tabs: the buttons (Open
  // PDF ▾: Reveal in Finder; Open ↗ its page; Copy [@key]; Edit), its status in words (✓ its .bib), its title and
  // authors with the first page of its PDF beside (Preview), its tags; of several selected, their tags.
  import { invoke } from "@tauri-apps/api/core";
  import { Menu, MenuItem } from "@tauri-apps/api/menu";
  import Marks from "$lib/feeds/Marks.svelte";
  import { mac } from "$lib/keys";
  import { fileName } from "$lib/library/filters";
  import Preview from "$lib/Preview.svelte";
  import type { Editor } from "$lib/library/editor.svelte";
  import Tags from "$lib/library/Tags.svelte";
  import type { Item, Row, TagCount } from "$lib/types";

  let {
    item,
    rows,
    tags,
    filtered,
    ontag,
    onset,
    onopen,
    editor,
    onedit,
    onsave,
    oncancel,
    onaddpdf,
  }: {
    item: Item;
    /** the papers the tags change: the selection when it has several, else the paper */
    rows: Row[];
    tags: TagCount[];
    /** whether the search filters a tag */
    filtered: (tag: string) => boolean;
    ontag: (tag: string, alternative: boolean) => void;
    onset: (add: string[], remove: string[]) => Promise<boolean>;
    onopen: () => void;
    /** the BibTeX being edited */
    editor: Editor;
    onedit: () => void;
    onsave: () => void;
    oncancel: () => void;
    /** add a PDF to the paper: the sheet, with the chooser of the PDF */
    onaddpdf: () => void;
  } = $props();

  let copied = $state(false);
  let hasPdf = $derived(item.files.some((f) => f.toLowerCase().endsWith(".pdf")));

  $effect(() => {
    item.id;
    copied = false;
  });

  async function copyKey() {
    await invoke("copy", { id: item.id, format: "pandoc" });
    copied = true;
  }

  /** The menu of Open PDF's ▾: the file in the Finder; a PDF to add, when the files are none. */
  async function fileMenu() {
    const reveal = await MenuItem.new({ text: mac ? "Reveal in Finder" : "Show in Folder", action: () => invoke("reveal", { id: item.id }) });
    const add = hasPdf ? [] : [await MenuItem.new({ text: "Add PDF…", action: onaddpdf })];
    await (await Menu.new({ items: [reveal, ...add] })).popup();
  }
</script>

<div class="details">
  <!-- the actions of one paper -->
  {#if rows.length <= 1}
    <div class="actions">
      {#if item.files.length}
        <span class="split">
          <button class="primary" onclick={onopen} title="Open the PDF (Space)">Open PDF</button>
          <button class="primary caret" aria-label="More" onclick={fileMenu}>▾</button>
        </span>
      {:else}
        <button onclick={onaddpdf} title="Add a PDF to this paper, or drop one on the page">Add PDF…</button>
      {/if}
      {#if item.doi || item.url}
        <button onclick={() => invoke("open_link", { id: item.id })} title="Open its page in the browser">Open ↗</button>
      {/if}
      <button onclick={copyKey}>{copied ? "Copied" : "Copy [@key]"}</button>
      <button onclick={onedit} disabled={editor.id === item.id}>Edit</button>
    </div>
    <Preview {item} marked={item.authors.map(() => false)} {onopen} keywords={false} {editor} {onsave} {oncancel} {onaddpdf}>
      {#snippet status()}
        {@const bib = fileName(item.bib)}
        <Marks marks={[{ slot: "library", symbol: "library", text: `in ${bib}`, entries: [{ id: item.id, bib }] }]} look="words" />
      {/snippet}
      {#snippet after()}{#if rows.length}<Tags {rows} {tags} {filtered} ontoggle={ontag} {onset} />{/if}{/snippet}
    </Preview>
  {:else}
    <h4>{rows.length} papers selected</h4>
    <div class="several"><Tags {rows} {tags} {filtered} ontoggle={ontag} {onset} /></div>
  {/if}
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
  .several {
    padding: 10px 16px 0;
  }
  h4 {
    margin: 14px 16px 0;
    font-size: 12px;
    font-weight: 600;
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
  button:disabled {
    opacity: 0.4;
  }
  button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
  .split {
    display: inline-flex;
  }
  .split button:first-child {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }
  .split .caret {
    padding: 3px 6px;
    border-left-color: color-mix(in srgb, currentColor 30%, transparent);
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
  }
</style>
