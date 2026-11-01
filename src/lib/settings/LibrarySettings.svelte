<script lang="ts">
  // The Library tab of the settings window: the papers folder, kept once set; the .bib files of the library, each with
  // what writes it, and how more come in: a new one made in the welcome window, a .bib used as it is, a papers library
  // opened (library.rs), the entries of a file imported into one of them; the Downloads panel of the main window and
  // its folder, also set there (`downloads`).
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import { mac } from "$lib/keys";
  import { writers as listWriters } from "$lib/library/writes";
  import type { Status, Writer } from "$lib/types";
  import "./settings.css";
  import { fileName, imported, type ImportReport, type Preferences, type View } from "./settings.svelte";

  let { prefs }: { prefs: Preferences } = $props();
  let settings = $derived(prefs.settings!);

  /** how each .bib file is written: by papers-rs, by a papers command, or by the app */
  let writers = $state<Writer[]>([]);

  /** a file to import, while the .bib it goes into is chosen */
  let importing = $state<{ file: string; bib: string } | null>(null);

  onMount(() => {
    listWriters().then((w) => (writers = w));
    const unlisten = [
      // the panel hidden, or its folder chosen, in the main window
      listen("downloads", syncDownloads),
      // the library read again: a .bib added, or a papers library opened
      listen<Status>("status", (e) => e.payload.state === "ready" && listWriters().then((w) => (writers = w))),
    ];
    return () => unlisten.forEach((u) => u.then((f) => f()));
  });

  function writtenBy(file: string): Writer | undefined {
    const home = prefs.home;
    const path = home && file.startsWith("~/") ? home + file.slice(1) : file;
    return writers.find((w) => w.bib === path);
  }

  async function addBibs() {
    const picked = await open({
      multiple: true,
      title: "Add .bib files",
      filters: [{ name: "BibTeX", extensions: ["bib"] }],
    });
    if (!picked) return;
    const added = (Array.isArray(picked) ? picked : [picked]).filter((file) => !settings.bibtex.includes(file));
    if (added.length) await prefs.save({ bibtex: [...settings.bibtex, ...added] });
  }

  /** The papers folder, while there is none. */
  async function choosePapersFolder() {
    const folder = await open({ directory: true, title: "The folder of your main library" });
    if (typeof folder === "string") await prefs.save({ papers_folder: folder });
  }

  /** A papers library brought in: its .bib joins the list (as the settings change, `follow`). */
  async function openLibrary() {
    const folder = await open({
      directory: true,
      title: "A papers library",
      defaultPath: settings.papers_folder ? prefs.expand(settings.papers_folder) : undefined,
    });
    if (typeof folder !== "string") return;
    try {
      const bib = await invoke<string>("library_open", { folder });
      prefs.show(`${fileName(bib)} is in the library`, false);
    } catch (err) {
      prefs.show(String(err), true);
    }
  }

  /** A file to import: into the .bib of the library, or the one chosen when there are several. */
  async function chooseImport() {
    const file = await open({
      title: "Import into the library",
      filters: [{ name: "BibTeX, RIS or CSL-JSON", extensions: ["bib", "ris", "json"] }],
    });
    if (typeof file !== "string") return;
    if (!settings.bibtex.length) return prefs.show("No .bib file to import into: add one first", true);
    if (settings.bibtex.length === 1) return importInto(file, settings.bibtex[0]);
    importing = { file, bib: settings.bibtex[0] };
  }

  /** Import the entries of `file` into `bib`; what was done said at the foot, until the next. */
  async function importInto(file: string, bib: string) {
    importing = null;
    prefs.show(`Importing ${fileName(file)}…`, false, true);
    try {
      const report = await invoke<ImportReport>("library_import", { bib: prefs.expand(bib), file });
      prefs.show(imported(report, bib), false, true);
    } catch (err) {
      prefs.show(String(err), true);
    }
  }

  /** The settings of the Downloads panel as they are now. */
  async function syncDownloads() {
    const { settings: now } = await invoke<View>("get_settings");
    settings.downloads_panel = now.downloads_panel;
    settings.downloads_folder = now.downloads_folder;
  }

  async function chooseDownloads() {
    const folder = await open({
      directory: true,
      title: "The folder of the Downloads panel",
      defaultPath: settings.downloads_folder.replace(/^~(?=\/)/, prefs.home ?? "~"),
    });
    if (typeof folder === "string") await prefs.save({ downloads_folder: folder });
  }

  /** The folder of a file, with ~ for the home folder. */
  function folder(path: string): string {
    const home = prefs.home;
    const dir = path.slice(0, path.length - fileName(path).length - 1);
    return home && dir.startsWith(home) ? "~" + dir.slice(home.length) : dir;
  }
</script>

<section>
  <h2>Library</h2>
  <div class="row">
    <span>
      <span class="label">Papers folder</span>
      {#if settings.papers_folder}
        <span class="hint" title={settings.papers_folder}>{prefs.tilde(settings.papers_folder)}</span>
      {/if}
      <span class="hint">The folder of your main library: a new .bib goes there by default</span>
    </span>
    {#if settings.papers_folder}
      {@const papers = prefs.expand(settings.papers_folder)}
      <button onclick={() => revealItemInDir(papers).catch((err) => prefs.show(String(err), true))}
        >{mac ? "Reveal in Finder" : "Show in Folder"}</button
      >
    {:else}
      <button onclick={choosePapersFolder}>Choose…</button>
    {/if}
  </div>
  <ul class="files">
    {#each settings.bibtex as file (file)}
      <li>
        <span class="file" title={file}>
          <span>{fileName(file)}</span>
          <span class="path">{folder(file)}</span>
          {#if writtenBy(file)}
            {@const writer = writtenBy(file)!}
            <span class="path" class:error={writer.kind === "papers-missing" || writer.problem !== null}>
              {writer.problem ??
                (writer.kind === "papers-rs"
                  ? "A papers library, written by papers-rs (built in)"
                  : writer.kind === "papers"
                    ? `A papers library, written by papers ${writer.version}`
                    : writer.kind === "native"
                      ? "Written by the app"
                      : "A papers library: read-only until the papers command (3.0.0b4 or later) is found")}
            </span>
          {/if}
        </span>
        <button
          class="remove"
          title="Remove from the library"
          onclick={() =>
            prefs.save({
              bibtex: settings.bibtex.filter((f) => f !== file),
              // added again later, it counts in the score's likeness, as any .bib added
              similar_skip: settings.similar_skip.filter((f) => f !== file),
            })}>✕</button
        >
      </li>
    {:else}
      <li class="none">No .bib file: add the files of your library.</li>
    {/each}
  </ul>
  <div class="buttons">
    <button onclick={() => invoke("open_welcome", { step: "new" })}>New .bib…</button>
    <button onclick={addBibs}>Add .bib Files…</button>
    <button onclick={openLibrary}>Open Papers Library…</button>
    <button onclick={chooseImport}>Import…</button>
  </div>
  {#if importing}
    {@const file = importing.file}
    <div class="row">
      <span class="label" title={file}>Import {fileName(file)} into</span>
      <span class="buttons">
        <select bind:value={importing.bib}>
          {#each settings.bibtex as bib (bib)}<option value={bib}>{fileName(bib)}</option>{/each}
        </select>
        <button class="primary" onclick={() => importInto(file, importing!.bib)}>Import</button>
        <button onclick={() => (importing = null)}>Cancel</button>
      </span>
    </div>
  {/if}
  <p class="hint">
    The panel searches them all, and reloads one when it changes. A papers library is a folder made by papers; an
    import adds the entries of a .bib, RIS or CSL-JSON file (Zotero, Mendeley and EndNote export them) to a .bib.
  </p>
</section>

<section>
  <h2>Downloads</h2>
  <label class="row">
    <span>
      <span class="label">Downloads panel</span>
      <span class="hint"
        >Under the sidebar: the PDFs of a folder outside the library, to drag into it. Only the names, sizes and
        dates of that folder are read.</span
      >
    </span>
    <input
      type="checkbox"
      class="switch"
      checked={settings.downloads_panel}
      onchange={(e) => prefs.save({ downloads_panel: e.currentTarget.checked })}
    />
  </label>
  {#if settings.downloads_panel}
    <div class="row">
      <span>
        <span class="label">Downloads folder</span>
        <span class="hint" title={settings.downloads_folder}>{prefs.tilde(settings.downloads_folder)}</span>
      </span>
      <button onclick={chooseDownloads}>Choose…</button>
    </div>
  {/if}
</section>
