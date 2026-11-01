<script lang="ts">
  // The welcome window (welcome_window in lib.rs). At a start without a library, its first step (`start`): what the
  // app does, and how a library comes in: a new one made (`new`), a papers library opened, a .bib used as it is, or a
  // file of another program imported into a new one; then the email and key for OpenAlex (`details`), which may wait,
  // and what was made (`done`). Later, New .bib… opens it at `new`: the form, then what was made. Its URL names the
  // step ("welcome#new"); the app names another while it is open (`step`). The fields and rows are those of the
  // settings window (settings.css).
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, tick } from "svelte";
  import "$lib/theme.css";
  import { hotkeyLabel, matches, zoomStep } from "$lib/keys";
  import { fileName, imported, Preferences, type ImportReport } from "$lib/settings/settings.svelte";
  import "$lib/settings/settings.css";
  import NewLibrary, { type Defaults } from "$lib/welcome/NewLibrary.svelte";

  type Step = "start" | "new" | "details" | "done";

  const prefs = new Preferences();
  let defaults = $state<Defaults | null>(null);
  let step = $state<Step>("start");
  /** opened at the start: the steps of the first start (Details, the hotkey said at the end); else New, then Done */
  let firstRun = $state(true);
  /** the file to import into the library made, chosen at the start */
  let toImport = $state<string | null>(null);
  /** what came into the library, for Done: how, its .bib, the import's report or why it failed */
  let made = $state<{ how: string; bib: string; report: string | null; problem: string | null } | null>(null);
  /** why a choice of the start failed */
  let problem = $state<string | null>(null);

  /** what Details saves: the fields as typed */
  let email = $state("");
  let openalexKey = $state("");

  let hotkey = $derived(prefs.settings ? hotkeyLabel(prefs.settings.hotkey) : "");

  onMount(() => {
    load();
    const unlisten = [listen<string>("step", (e) => begin(e.payload)), prefs.follow()];
    return () => unlisten.forEach((u) => u.then((f) => f()));
  });

  /** Fill the window, then show it: it is made hidden, not to flash empty. */
  async function load() {
    await prefs.load();
    try {
      defaults = await invoke<Defaults>("library_defaults");
    } catch (err) {
      problem = String(err);
    }
    email = prefs.settings?.email ?? "";
    openalexKey = prefs.settings?.openalex_key ?? "";
    begin(location.hash.slice(1));
    await tick();
    const window = getCurrentWindow();
    await window.show();
    await window.setFocus();
  }

  /** Begin again at a step the app names: "new", else the start. */
  function begin(named: string) {
    step = named === "new" ? "new" : "start";
    firstRun = step === "start";
    toImport = null;
    made = null;
    problem = null;
  }

  /** The library is in: its details at the first start, else what was made. The settings read again first, as the
   *  command changed them: Details saves them. */
  async function after(how: string, bib: string, report: string | null = null, problem: string | null = null) {
    await prefs.load();
    made = { how, bib, report, problem };
    step = firstRun ? "details" : "done";
  }

  /** The library made at New, the file chosen at the start imported into it, New waiting. */
  async function created(bib: string) {
    if (!toImport) return after("A new library", bib);
    try {
      const report = await invoke<ImportReport>("library_import", { bib, file: toImport });
      await after("A new library", bib, imported(report, bib));
    } catch (err) {
      await after("A new library", bib, null, `The import failed: ${err}`);
    }
  }

  /** A papers library brought in as it is. */
  async function openLibrary() {
    problem = null;
    const folder = await open({ directory: true, title: "A papers library", defaultPath: defaults?.papersFolder });
    if (typeof folder !== "string") return;
    try {
      await after("A papers library", await invoke<string>("library_open", { folder }));
    } catch (err) {
      problem = String(err);
    }
  }

  /** A .bib file used where it is, as the settings' Add .bib Files… adds it. */
  async function useBib() {
    problem = null;
    const file = await open({ title: "A .bib file", filters: [{ name: "BibTeX", extensions: ["bib"] }] });
    if (typeof file !== "string" || !prefs.settings) return;
    const bibtex = prefs.settings.bibtex;
    if (bibtex.includes(file) || (await prefs.save({ bibtex: [...bibtex, file] }))) await after("A .bib file", file);
    else problem = prefs.notice?.text ?? null;
  }

  /** A file of another program to import: chosen now, imported into the library New makes. */
  async function chooseImport() {
    problem = null;
    const file = await open({
      title: "Import into a new library",
      filters: [{ name: "BibTeX, RIS or CSL-JSON", extensions: ["bib", "ris", "json"] }],
    });
    if (typeof file !== "string") return;
    toImport = file;
    step = "new";
  }

  /** The details as typed, saved; what is unchanged left as it is. */
  async function saveDetails() {
    const change = { email: email.trim(), openalex_key: openalexKey.trim() };
    const settings = prefs.settings!;
    if (change.email === settings.email && change.openalex_key === settings.openalex_key) step = "done";
    else if (await prefs.save(change)) step = "done";
  }

  async function openTheLibrary() {
    await invoke("open_library");
    getCurrentWindow().close();
  }

  /** ⌘W closes the window, ⌘+ and ⌘- change the text size. */
  function onkeydown(e: KeyboardEvent) {
    if (matches(e, { key: "w", mod: true })) getCurrentWindow().close();
    const zoom = zoomStep(e);
    if (zoom !== null) {
      e.preventDefault();
      invoke("text_size_step", { steps: zoom });
    }
  }
</script>

<svelte:window {onkeydown} />

{#if prefs.settings}
  <main class="settings welcome">
    {#if step === "start"}
      <h1>Welcome to Papers Desktop</h1>
      <ul class="cards">
        <li>
          <span class="label">Library</span>
          <span class="hint">Your .bib files and their PDFs. <kbd>{hotkey}</kbd> shows the panel, which finds any paper from any app.</span>
        </li>
        <li>
          <span class="label">Feeds</span>
          <span class="hint">The journals and searches you follow, ranked by your interests.</span>
        </li>
        <li>
          <span class="label">Search</span>
          <span class="hint">All of OpenAlex: add what you find.</span>
        </li>
      </ul>
      <section>
        <h2>Your library</h2>
        <button class="choice" disabled={!defaults} onclick={() => (step = "new")}>
          <span class="label">Start a new library</span>
          <span class="hint">A .bib file and a folder for its PDFs</span>
        </button>
        <button class="choice" onclick={openLibrary}>
          <span class="label">Open a papers library…</span>
          <span class="hint">A folder papers made: its .bib and PDFs as they are</span>
        </button>
        <button class="choice" onclick={useBib}>
          <span class="label">Use a .bib file as it is…</span>
          <span class="hint">Read where it is, and written there by the app</span>
        </button>
        <button class="choice" disabled={!defaults} onclick={chooseImport}>
          <span class="label">Import a .bib or RIS file into a new library…</span>
          <span class="hint">Zotero, Mendeley, EndNote… export one: its entries and PDFs copied into the new library</span>
        </button>
      </section>
      {#if problem}<p class="hint error">{problem}</p>{/if}
    {:else if step === "new" && defaults}
      <NewLibrary
        {prefs}
        {defaults}
        {firstRun}
        importing={toImport}
        oncreated={created}
        onback={() => (firstRun ? begin("start") : getCurrentWindow().close())}
      />
    {:else if step === "details"}
      <h1>Two details</h1>
      <p class="intro">Both optional, and in the settings (General) later.</p>
      <section>
        <label class="row">
          <span>
            <span class="label">Email</span>
            <span class="hint">Sent to doi.org and OpenAlex, which answer faster to programs that give one</span>
          </span>
          <input class="text" type="email" placeholder="you@example.org" spellcheck="false" autocapitalize="off" bind:value={email} />
        </label>
        <label class="row">
          <span>
            <span class="label">OpenAlex API key</span>
            <span class="hint">Free key from openalex.org: more searches a day than without</span>
          </span>
          <input class="text" type="password" autocomplete="off" spellcheck="false" bind:value={openalexKey} />
        </label>
      </section>
      {#if prefs.notice?.error}<p class="hint error">{prefs.notice.text}</p>{/if}
      <div class="actions">
        <button onclick={() => (step = "done")}>Skip</button>
        <button class="primary" onclick={saveDetails}>Continue</button>
      </div>
    {:else if step === "done" && made}
      <h1>{firstRun ? "Ready" : "Made"}</h1>
      <section>
        <div class="row">
          <span>
            <span class="label">{made.how}: {fileName(made.bib)}</span>
            <span class="hint" title={made.bib}>{prefs.tilde(made.bib)}</span>
            {#if made.report}<span class="hint">{made.report}</span>{/if}
            {#if made.problem}<span class="hint error">{made.problem}</span>{/if}
          </span>
        </div>
      </section>
      {#if firstRun}
        <p class="intro">Press <kbd>{hotkey}</kbd> in any app to find a paper. The settings are in the menu-bar icon.</p>
      {/if}
      <div class="actions">
        <button onclick={() => getCurrentWindow().close()}>Close</button>
        <button class="primary" onclick={openTheLibrary}>Open the Library</button>
      </div>
    {/if}
  </main>
{/if}

<style>
  :global(html, body) {
    margin: 0;
    background: var(--bg);
    color: var(--fg);
    font-size: 13px;
    cursor: default;
    -webkit-user-select: none;
    user-select: none;
  }

  /* a step: its heading, its cards, its buttons at the foot */
  main {
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 12px;
    height: 100vh;
    overflow-y: auto;
    padding: 20px 26px;
  }
  main :global(h1) {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
  }
  main :global(.intro) {
    margin: 0;
    color: var(--muted);
  }
  main :global(p.hint) {
    margin: 0;
  }
  main :global(.actions) {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: auto;
  }
  kbd {
    font: inherit;
    font-weight: 600;
    color: var(--fg);
  }

  /* what the app does: three cards in a row */
  .cards {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .cards li {
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--menu-bg);
  }
  .cards .label {
    font-weight: 600;
  }

  /* a way to a library: a button the width of the card, its hint under it */
  .choice {
    display: block;
    width: 100%;
    margin: 6px 0;
    padding: 8px 12px;
    text-align: left;
  }
  .choice:hover:not(:disabled) {
    border-color: var(--accent);
  }
</style>
