<script lang="ts">
  // The Library tab of the settings window: the .bib files of the library, each with what writes it; the Downloads
  // panel of the main window and its folder, also set there (`downloads`).
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { writers as listWriters } from "$lib/library/writes";
  import type { Writer } from "$lib/types";
  import "./settings.css";
  import { fileName, type Preferences, type View } from "./settings.svelte";

  let { prefs }: { prefs: Preferences } = $props();
  let settings = $derived(prefs.settings!);

  /** how each .bib file is written: by papers-rs, by a papers command, or by the app */
  let writers = $state<Writer[]>([]);

  onMount(() => {
    listWriters().then((w) => (writers = w));
    // the panel hidden, or its folder chosen, in the main window
    const unlisten = listen("downloads", syncDownloads);
    return () => unlisten.then((f) => f());
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
          onclick={() => prefs.save({ bibtex: settings.bibtex.filter((f) => f !== file) })}>✕</button
        >
      </li>
    {:else}
      <li class="none">No .bib file: add the files of your library.</li>
    {/each}
  </ul>
  <button onclick={addBibs}>Add .bib Files…</button>
  <p class="hint">The panel searches them all, and reloads one when it changes.</p>
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
