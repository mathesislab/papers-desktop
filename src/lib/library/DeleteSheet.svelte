<script lang="ts">
  // The question before papers are deleted from the library: "Delete 3 papers?", with each paper (its key and
  // its citation in the style of the settings, as the preview shows it) and the files it links, each to
  // be deleted or not (not, to start with). What becomes of a file depends on how the .bib is written: a
  // papers library keeps it in its history, so that Undo brings it back; from a plain .bib it goes to the Trash.

  import { onMount } from "svelte";
  import { holdModal } from "./modal.svelte";
  import * as cite from "$lib/cite";
  import { fileName } from "./filters";
  import { count } from "./selection";

  let {
    papers,
    note,
    ondelete,
    oncancel,
  }: {
    papers: { id: string; key: string; files: string[] }[];
    /** where the files go when deleted */
    note: string;
    /** go on, with these files deleted */
    ondelete: (files: string[]) => void;
    oncancel: () => void;
  } = $props();

  /** the files to delete, by path: a file two papers link is one choice */
  let ticked = $state<Record<string, boolean>>({});
  let files = $derived(Object.keys(ticked).filter((f) => ticked[f]));
  let confirm = $state<HTMLButtonElement>();

  $effect(() => confirm?.focus());

  onMount(holdModal);
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && oncancel()} />

<div class="backdrop">
  <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="delete-title">
    <h2 id="delete-title">Delete {count(papers.length)}?</h2>
    <p>{papers.length === 1 ? "It is" : "They are"} removed from the library; Undo takes {papers.length === 1 ? "it" : "them"} back.</p>
    <div class="list">
      {#each papers as paper (paper.id)}
        <div class="paper">
          <div class="key">{paper.key}</div>
          <!-- citeproc-js escapes the fields of the entry -->
          {#await cite.citation(paper.id) then html}
            {#if html}<div class="citation">{@html html}</div>{/if}
          {:catch}
            <!-- no citation: the key says which paper -->
          {/await}
          {#each paper.files as file (file)}
            <label title={file}>
              <input type="checkbox" bind:checked={ticked[file]} />
              <span class="file">{fileName(file)}</span>
            </label>
          {:else}
            <div class="nofile">(no PDF)</div>
          {/each}
        </div>
      {/each}
    </div>
    {#if files.length}<p class="note">{note}</p>{/if}
    <div class="actions">
      <button onclick={oncancel}>Cancel</button>
      <button class="primary" bind:this={confirm} onclick={() => ondelete(files)}>
        Delete {count(papers.length)}{files.length ? `, ${count(files.length, "file")}` : ""}
      </button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.25);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    width: min(520px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    padding: 18px 20px 16px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--menu-bg);
    color: var(--fg);
    font-size: 13px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.22);
  }
  h2 {
    margin: 0 0 10px;
    font-size: 14px;
    font-weight: 600;
  }
  p {
    margin: 0 0 10px;
    line-height: 1.45;
  }
  .list {
    min-height: 0;
    margin-bottom: 4px;
    overflow-y: auto;
  }
  .paper {
    padding: 6px 0;
    border-top: 1px solid var(--line);
  }
  .key {
    color: var(--muted);
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    font-size: 11.5px;
  }
  .citation {
    margin: 2px 0 4px;
    font-size: 12.5px;
    line-height: 1.45;
  }
  /* the numbers of numbered styles */
  .citation :global(.csl-left-margin) {
    display: none;
  }
  .citation :global(.csl-right-inline) {
    display: inline;
  }
  .nofile {
    color: var(--muted);
    font-size: 12px;
  }
  .note {
    margin: 6px 0 0;
    color: var(--muted);
    font-size: 12px;
  }
  label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }
  .file {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .actions {
    display: flex;
    flex: none;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 14px;
  }
  button {
    flex: none;
    padding: 3px 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
</style>
