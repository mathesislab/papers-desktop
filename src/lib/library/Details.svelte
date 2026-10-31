<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { mac } from "$lib/keys";
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
</script>

<div class="details">
  <!-- the actions of one paper -->
  {#if rows.length <= 1}
    <div class="actions">
      {#if item.files.length || item.doi || item.url}
        <button class="primary" onclick={onopen}>{item.files.length ? "Open PDF" : "Open DOI page"}</button>
      {/if}
      {#if item.files.length}
        <button onclick={() => invoke("reveal", { id: item.id })}>{mac ? "Reveal in Finder" : "Show in folder"}</button>
      {/if}
      {#if !hasPdf}
        <button onclick={onaddpdf} title="Add a PDF to this paper, or drop one on the page below">Add PDF…</button>
      {/if}
      <button onclick={copyKey}>{copied ? "Copied" : "Copy [@key]"}</button>
      <button onclick={onedit} disabled={editor.id === item.id}>Edit</button>
    </div>
  {/if}
  {#if rows.length > 1}
    <h4>{rows.length} papers selected</h4>
  {/if}
  {#if rows.length}
    <Tags {rows} {tags} {filtered} ontoggle={ontag} {onset} />
  {/if}
  {#if rows.length <= 1}
    <Preview {item} marked={item.authors.map(() => false)} {onopen} keywords={false} {editor} {onsave} {oncancel} {onaddpdf} />
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
</style>
