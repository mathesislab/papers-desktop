<script lang="ts">
  // The question before papers are deleted from the library: "Delete 3 papers?", with a choice to delete their
  // PDFs too (off). What becomes of the PDFs depends on how the .bib is written: a papers library keeps them in
  // its history, so that Undo brings them back; from a plain .bib they go to the Trash.

  import { count } from "./selection";

  let {
    papers,
    title,
    files,
    note,
    ondelete,
    oncancel,
  }: {
    /** how many papers */
    papers: number;
    /** of the paper, when there is one */
    title: string | null;
    /** whether any of them has a file: what the choice is about */
    files: boolean;
    /** where the PDFs go when deleted */
    note: string;
    /** go on, with their PDFs deleted or not */
    ondelete: (deleteFiles: boolean) => void;
    oncancel: () => void;
  } = $props();

  let deleteFiles = $state(false);
  let confirm = $state<HTMLButtonElement>();

  $effect(() => confirm?.focus());
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && oncancel()} />

<div class="backdrop">
  <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="delete-title">
    <h2 id="delete-title">Delete {count(papers)}?</h2>
    {#if title}<p class="paper">{title}</p>{/if}
    <p>{papers === 1 ? "It is" : "They are"} removed from the library; Undo takes {papers === 1 ? "it" : "them"} back.</p>
    {#if files}
      <label>
        <input type="checkbox" bind:checked={deleteFiles} />
        Also delete {papers === 1 ? "its" : "their"} PDFs
      </label>
      {#if deleteFiles}<p class="note">{note}</p>{/if}
    {/if}
    <div class="actions">
      <button onclick={oncancel}>Cancel</button>
      <button class="primary" bind:this={confirm} onclick={() => ondelete(deleteFiles)}>Delete</button>
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
    width: min(400px, calc(100vw - 32px));
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
  .paper {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-style: italic;
  }
  .note {
    margin: 6px 0 0 22px;
    color: var(--muted);
    font-size: 12px;
  }
  label {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .actions {
    display: flex;
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
