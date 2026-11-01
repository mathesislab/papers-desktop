<script lang="ts" module>
  /** mirrors library::LibraryDefaults */
  export type Defaults = { papersFolder: string; mainLibrary: string | null };
</script>

<script lang="ts">
  // The New step of the welcome window: a new papers library (library_create), its name, its folder, where its PDFs
  // go. The first library, the main one, goes in the papers folder itself; a later one in a folder of its name there,
  // which follows the name until one is chosen by hand. Its PDFs in a folder of its own (`files` in the library's,
  // unless chosen), or shared with the main library, which keeps them: none is copied. A refusal is said under the
  // fields, which stay as typed.
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { fileName, type Preferences } from "$lib/settings/settings.svelte";

  let {
    prefs,
    defaults,
    firstRun,
    importing,
    oncreated,
    onback,
  }: {
    prefs: Preferences;
    defaults: Defaults;
    /** at the first start: "papers" the name to begin with */
    firstRun: boolean;
    /** a file to import into the library once made, to say so */
    importing: string | null;
    /** the library made, its .bib; what follows (an import) done before the form is free again */
    oncreated: (bib: string) => Promise<void>;
    /** back to the step before, or the window closed */
    onback: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let name = $state(firstRun ? "papers" : "");
  /** the folders chosen by hand; none: the defaults */
  let pickedFolder = $state<string | null>(null);
  let pickedFiles = $state<string | null>(null);
  /** the PDFs shared with the main library, when it may be */
  let shared = $state(false);
  let busy = $state(false);
  let problem = $state<string | null>(null);

  /** the name as the .bib is named, without its extension */
  let base = $derived(name.trim().replace(/\.bib$/, ""));
  /** no main library yet: this one is it, in the papers folder */
  let main = $derived(!prefs.settings?.papers_folder);
  let folder = $derived(pickedFolder ?? (main ? defaults.papersFolder : `${defaults.papersFolder}/${base}`));
  let files = $derived(pickedFiles ?? `${folder}/files`);
  /** the main library, to share its PDFs: not this folder's own */
  let canShare = $derived(defaults.mainLibrary !== null && defaults.mainLibrary !== folder);
  let pool = $derived(shared && canShare);

  async function chooseFolder() {
    const picked = await open({ directory: true, title: "The folder of the new library", defaultPath: folder });
    if (typeof picked === "string") pickedFolder = picked;
  }

  async function chooseFiles() {
    const picked = await open({ directory: true, title: "The folder of its PDFs", defaultPath: files });
    if (typeof picked === "string") pickedFiles = picked;
  }

  async function create() {
    busy = true;
    problem = null;
    try {
      const bib = await invoke<string>("library_create", {
        folder,
        bib: base,
        files: pool ? { kind: "pool" } : { kind: "folder", path: files },
      });
      await oncreated(bib);
    } catch (err) {
      problem = String(err);
    } finally {
      busy = false;
    }
  }
</script>

<h1>{firstRun ? "A new library" : "New .bib"}</h1>
<p class="intro">A papers library: a .bib file and the PDFs of its papers, in a folder of their own. The app writes it, and so can the papers command.</p>

<section>
  <label class="row">
    <span>
      <span class="label">Name</span>
      <span class="hint">The .bib file is {base || "…"}.bib</span>
    </span>
    <!-- svelte-ignore a11y_autofocus -->
    <input class="text" bind:value={name} placeholder="papers" spellcheck="false" autocapitalize="off" autofocus />
  </label>
  <div class="row">
    <span>
      <span class="label">Folder</span>
      <span class="hint" title={folder}>{prefs.tilde(folder)}</span>
    </span>
    <button onclick={chooseFolder}>Choose…</button>
  </div>
  <div class="row pdfs">
    <span class="label">PDFs</span>
    <span class="choices">
      <label class="choice">
        {#if canShare}<input type="radio" name="pdfs" checked={!pool} onchange={() => (shared = false)} />{/if}
        <span>
          <span class="label">A folder of its own</span>
          <span class="hint" title={files}>{prefs.tilde(files)}</span>
        </span>
        <button disabled={pool} onclick={chooseFiles}>Choose…</button>
      </label>
      {#if canShare}
        <label class="choice">
          <input type="radio" name="pdfs" checked={pool} onchange={() => (shared = true)} />
          <span>
            <span class="label">Shared with the main library</span>
            <span class="hint">No PDF is copied: they stay with the main library, in {prefs.tilde(defaults.mainLibrary!)}</span>
          </span>
        </label>
      {/if}
    </span>
  </div>
</section>
{#if importing}
  <p class="hint">Then the entries of {fileName(importing)} are imported into it, their PDFs copied.</p>
{/if}
{#if problem}
  <p class="hint error">{problem}</p>
{/if}

<div class="actions">
  <button onclick={onback}>{firstRun ? "Back" : "Cancel"}</button>
  <button class="primary" disabled={busy || !base} onclick={create}>
    {busy ? (importing ? "Importing…" : "Creating…") : "Create"}
  </button>
</div>

<style>
  .pdfs {
    align-items: flex-start;
  }
  .choices {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 360px;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .choice > span {
    flex: 1;
    min-width: 0;
  }
  .choice .hint {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .choice input {
    margin: 0;
    accent-color: var(--accent);
  }
</style>
