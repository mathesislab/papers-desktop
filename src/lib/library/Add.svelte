<script lang="ts">
  // Add a paper: from a PDF (its DOI is read, then its entry fetched), a DOI, the link of a
  // journal article, or BibTeX; with the PDF, which is moved, copied or left where it is.
  // The same sheet adds a PDF to a paper of the library (`forId`): the entry is that paper's.
  // The entry's BibTeX is in a field to fix before adding, and the papers of the library
  // that are the same are listed under it.

  import { invoke } from "@tauri-apps/api/core";
  import { downloadDir } from "@tauri-apps/api/path";
  import { open as pickFile } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import { label } from "$lib/keys";
  import { defaultFiles, writers } from "$lib/library/writes";
  import type { AddOptions, FilesMode, Incoming, Input, Item, Page, PdfRead, Row, Writer } from "$lib/types";

  let {
    bib: preferred = null,
    rows,
    forId = null,
    pdf: first = null,
    choose: choosing = false,
    onadd,
    onattach,
    onclose,
  }: {
    /** the .bib file to add to, unless the user chooses another: that of the paper selected */
    bib?: string | null;
    /** the papers of the library, by id */
    rows: Map<string, Row>;
    /** a paper of the library to add a PDF to, instead of a new paper */
    forId?: string | null;
    /** a PDF to start with: one dropped, or chosen */
    pdf?: string | null;
    /** open the chooser of the PDF at once */
    choose?: boolean;
    /** write the new paper; whether it was written, else `fail` was told why */
    onadd: (bib: string, text: string, options: AddOptions, fail: (message: string) => void) => Promise<boolean>;
    /** add the PDF to the paper, its entry first changed to `text` when that is not null */
    onattach: (
      id: string,
      text: string | null,
      pdf: string,
      files: FilesMode | null,
      fail: (message: string) => void,
    ) => Promise<boolean>;
    onclose: () => void;
  } = $props();

  const ADD = { key: "Enter", mod: true };
  const MODES: { mode: FilesMode; name: string; title: string }[] = [
    { mode: "move", name: "Move", title: "Into the library's folder of files, named by its template; the original goes" },
    { mode: "copy", name: "Copy", title: "Into the library's folder of files, named by its template; the original stays" },
    { mode: "link", name: "Leave in place", title: "The paper links the file where it is" },
  ];

  /** the field: a DOI, a link, the path of a PDF */
  let field = $state("");
  let input = $state<HTMLInputElement>();
  /** the BibTeX of the entry */
  let text = $state("");
  /** the PDF, and whether the app downloaded it (papers always places a download in the library) */
  let pdf = $state<{ path: string; downloaded: boolean } | null>(null);
  /** where the DOI in the field came from */
  let source = $state<"pdf" | "search" | "typed" | "page" | "entry" | "none" | null>(null);
  /** the article the entry is of, to open in a browser; whether a free PDF is known (undefined: being looked for) */
  let article = $state<{ page: string; free: string | null | undefined } | null>(null);
  /** the .bib files of the library, and their writers; the one to add to */
  let bibs = $state<Writer[]>([]);
  let target = $state("");
  /** for a papers library: what papers does with the papers already there */
  let onDuplicate = $state<AddOptions["onDuplicate"]>("skip");
  /** for a paper that has an entry already: how an entry fetched is merged into it */
  let merging = $state<"keep" | "update">("keep");
  /** what is done with the PDF */
  let mode = $state<FilesMode>("move");
  let modeChosen = false;
  /** the key doi.org gave the entry it fetched: while it stays, the library's key template makes the key */
  let fetchedKey = $state<string | null>(null);
  /** whether the entry has a DOI to say where it is from: the BibTeX folds */
  let doiKnown = $state(false);
  /** the BibTeX shown or folded by the user; none: as it comes */
  let folded = $state<boolean | null>(null);
  /** what is being done, and what went wrong or is to know */
  let busy = $state<string | null>(null);
  let message = $state<{ text: string; error?: boolean } | null>(null);
  /** the title of the paper that gets the PDF */
  let paper = $state<Item | null>(null);
  /** the entries of the BibTeX, or why it does not parse */
  let checked = $state<{ entries: Incoming[] } | { error: string } | null>(null);
  let adding = $state(false);
  let error = $state<string | null>(null);
  /** the latest thing the user started: what an older one finds out is dropped */
  let seq = 0;

  let entries = $derived(checked && "entries" in checked ? checked.entries : []);
  /** the papers of the library the entries are the same as, but the one that gets the PDF */
  let known = $derived.by(() => {
    const ids = [...new Set(entries.flatMap((e) => e.same))].filter((id) => id !== forId);
    return ids.map((id) => rows.get(id)).filter((r): r is Row => r !== undefined);
  });
  let kind = $derived(bibs.find((w) => w.bib === target)?.kind);
  /** whether papers writes the .bib file (papers-rs or a papers command): it merges the papers already there, makes keys, places files */
  let papers = $derived(kind === "papers-rs" || kind === "papers");
  let native = $derived(kind === "native");
  let canAdd = $derived(
    entries.length > 0 && !adding && !busy && target !== "" && (forId === null || pdf !== null),
  );
  let open = $derived(!(folded ?? doiKnown));
  let entry = $derived(entries.length === 1 ? entries[0].item : null);
  let file = $derived(pdf ? pdf.path.split(/[\\/]/).pop() : null);
  let folder = $derived(pdf ? pdf.path.split(/[\\/]/).slice(0, -1).join("/").replace(/^\/Users\/[^/]+/, "~") : "");
  let sourceText = $derived.by(() => {
    switch (source) {
      case "pdf":
        return "from the PDF ✓";
      case "search":
        return "found by a search of the PDF's text";
      case "page":
        return "from the page ✓";
      case "entry":
        return "the paper's DOI";
      case "typed":
        return "typed";
      case "none":
        return "none found";
      default:
        return "";
    }
  });

  onMount(async () => {
    input?.focus();
    bibs = await writers();
    if (forId) {
      try {
        paper = await invoke<Item>("item", { id: forId });
        target = paper.bib;
        text = await invoke<string>("bibtex", { id: forId, raw: true });
        field = paper.doi ?? "";
        source = paper.doi ? "entry" : "none";
        doiKnown = !!paper.doi;
      } catch (e) {
        message = { text: String(e), error: true };
      }
    } else {
      target = preferred && bibs.some((w) => w.bib === preferred) ? preferred : (bibs[0]?.bib ?? "");
    }
    if (first) setPdf(first);
    else if (choosing) chooseFile();
  });

  // what is done with a file is the library's `files` setting, until the user chooses
  $effect(() => {
    const bib = target;
    if (!bib) return;
    defaultFiles(bib)
      .then((m) => {
        if (m && !modeChosen && bib === target) mode = m;
      })
      .catch(() => {});
  });

  // a downloaded file is always placed in the library, never left where it is
  $effect(() => {
    if (pdf?.downloaded && mode === "link") mode = "move";
  });

  // the entries of the BibTeX, and the papers they are the same as: a moment
  // after typing, and again when the library is read again
  $effect(() => {
    const bibtex = text.trim() === "" ? null : text;
    rows;
    if (bibtex === null) {
      checked = null;
      return;
    }
    let live = true;
    const timer = setTimeout(async () => {
      try {
        const found = await invoke<Incoming[]>("incoming", { text: bibtex });
        if (live) checked = { entries: found };
      } catch (e) {
        if (live) checked = { error: String(e) };
      }
    }, 200);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  });

  const fail = (e: unknown) => (message = { text: String(e), error: true });
  const keyOf = (bibtex: string) => /^\s*@\w+\s*\{\s*([^,\s]+)/.exec(bibtex)?.[1] ?? null;

  /** Use a PDF, as the one to add: dropped, chosen, written in the field, downloaded. */
  export async function setPdf(path: string, downloaded = false) {
    const n = ++seq;
    pdf = { path, downloaded };
    error = null;
    message = null;
    // the entry is there already, from a DOI or the paper: the PDF is for it, and checked against it
    if (text.trim() !== "" && !forId) return;
    await read(n);
  }

  /** The DOI of the PDF, as `papers add` finds it, and its entry: in place of the entry of the sheet. */
  async function read(n: number) {
    if (!pdf) return;
    busy = "Reading the PDF…";
    message = null;
    try {
      const found = await invoke<PdfRead>("read_pdf_doi", { path: pdf.path });
      if (n !== seq) return;
      if (forId) return check(found);
      article = null;
      if (found.doi) {
        field = found.doi;
        source = found.source === "search" ? "search" : "pdf";
      } else {
        field = "";
        source = "none";
      }
      if (found.bibtex) {
        text = found.bibtex;
        fetchedKey = keyOf(found.bibtex);
        doiKnown = true;
        folded = null;
      } else {
        doiKnown = false;
        message = { text: found.message ?? "No entry found", error: !found.doi };
        if (!found.doi) message.text += ": type its DOI, or write the BibTeX";
      }
    } catch (e) {
      if (n === seq) fail(e);
    } finally {
      if (n === seq) busy = null;
    }
  }

  /** A PDF for a paper that has its entry: the DOI the PDF has is the paper's, or it is told. */
  function check(found: PdfRead) {
    const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();
    if (found.doi && paper?.doi && !same(found.doi, paper.doi)) {
      message = { text: `The PDF has the DOI ${found.doi}, the paper ${paper.doi}`, error: true };
    }
  }

  /** The entry of the DOI in the field: in place of the BibTeX, or for a paper that has one, merged into it. */
  async function fetchEntry(doi: string, n: number): Promise<boolean> {
    busy = `Fetching ${doi}…`;
    message = null;
    try {
      const fetched = await invoke<string>("fetch_doi", { doi });
      if (n !== seq) return false;
      if (forId) {
        text = await invoke<string>("merge_fetched", { current: text, fetched, update: merging === "update" });
      } else {
        text = fetched;
        fetchedKey = keyOf(fetched);
      }
      doiKnown = true;
      return true;
    } catch (e) {
      if (n === seq) fail(e);
      return false;
    } finally {
      if (n === seq) busy = null;
    }
  }

  /** Whether OpenAlex lists a free PDF of the article. */
  async function lookup(doi: string, page: string, n: number) {
    if (pdf) return;
    article = { page, free: undefined };
    try {
      const free = await invoke<string | null>("free_pdf", { doi });
      if (n === seq && article) article = { page, free };
    } catch {
      if (n === seq && article) article = { page, free: null };
    }
  }

  /** The free PDF, downloaded into a folder of the temporary files, and made the PDF of the sheet. */
  async function download(url: string, n: number) {
    busy = "Downloading the PDF…";
    message = null;
    try {
      const path = await invoke<string>("download_pdf", { url });
      if (n !== seq) return;
      busy = null;
      await usePdf(path, n);
    } catch (e) {
      if (n === seq) fail(e);
    } finally {
      if (n === seq) busy = null;
    }
  }

  async function usePdf(path: string, n: number) {
    pdf = { path, downloaded: true };
    if (text.trim() === "") await read(n);
  }

  /** The DOI of the article a link leads to, then its entry. */
  async function openPage(url: string, n: number) {
    busy = "Reading the page…";
    message = null;
    try {
      const page = await invoke<Page>("journal_page", { url });
      if (n !== seq) return;
      if (page.pdf) return await download(url, n);
      if (!page.doi) {
        article = { page: page.url, free: null };
        source = "none";
        message = { text: "Journal article, no DOI found on its page: open it in the browser, or type the DOI" };
        return;
      }
      field = page.doi;
      source = "page";
      message = { text: `Journal article, DOI ${page.doi}` };
      if (await fetchEntry(page.doi, n)) {
        // (the line of the DOI stays)
        message = { text: `Journal article, DOI ${page.doi}` };
        await lookup(page.doi, page.url, n);
      }
    } catch (e) {
      if (n === seq) fail(e);
    } finally {
      if (n === seq) busy = null;
    }
  }

  /** What is in the field: a DOI is fetched, a PDF read, a link followed, BibTeX taken as the entry. */
  async function go() {
    const n = ++seq;
    let found: Input;
    try {
      found = await invoke<Input>("classify_input", { text: field });
    } catch (e) {
      return fail(e);
    }
    if (n !== seq) return;
    error = null;
    switch (found.kind) {
      case "empty":
        message = null;
        return;
      case "bibtex":
        text = field;
        field = "";
        source = null;
        doiKnown = false;
        fetchedKey = null;
        article = null;
        message = null;
        return;
      case "doi":
        field = found.doi;
        source = paper?.doi?.toLowerCase() === found.doi.toLowerCase() ? "entry" : "typed";
        article = null;
        if (await fetchEntry(found.doi, n)) await lookup(found.doi, `https://doi.org/${found.doi}`, n);
        return;
      case "pdf":
        field = "";
        pdf = { path: found.path, downloaded: false };
        if (text.trim() !== "" && !forId) return;
        return read(n);
      case "pdf-url":
        field = "";
        return download(found.url, n);
      case "url":
        return openPage(found.url, n);
      case "other":
        message = { text: found.message, error: true };
    }
  }

  /** The DOI is back to what the PDF gives. */
  async function again() {
    await read(++seq);
  }

  async function chooseFile() {
    try {
      const picked = await pickFile({
        title: "PDF to add",
        multiple: false,
        directory: false,
        defaultPath: await downloadDir().catch(() => undefined),
        filters: [{ name: "PDF", extensions: ["pdf"] }],
      });
      if (typeof picked === "string") await setPdf(picked);
    } catch (e) {
      fail(e);
    }
  }

  async function add() {
    if (!canAdd) return;
    adding = true;
    error = null;
    const files = pdf && !native ? mode : null;
    const fail = (m: string) => (error = m);
    if (forId) {
      await onattach(forId, text, pdf!.path, files, fail);
    } else {
      const updateKey = fetchedKey !== null && entries.length === 1 && entries[0].key === fetchedKey;
      await onadd(target, text, { onDuplicate, updateKey, pdf: pdf?.path ?? null, files }, fail);
    }
    adding = false;
  }

  /** BibTeX pasted in the field is the entry; any other text is looked at once. */
  function paste(e: ClipboardEvent) {
    const pasted = e.clipboardData?.getData("text") ?? "";
    if (pasted.trimStart().startsWith("@")) {
      e.preventDefault();
      field = pasted;
    }
    setTimeout(go);
  }

  function onkeydown(e: KeyboardEvent) {
    // the keys of the library (arrows, Enter, Escape) are not for the sheet
    e.stopPropagation();
    const mod = e.metaKey || e.ctrlKey;
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "Enter" && mod) {
      e.preventDefault();
      add();
    } else if (e.key === "Enter" && e.target === input) {
      e.preventDefault();
      go();
    }
  }

  const name = (bib: string) => bib.split(/[\\/]/).pop();
</script>

<!-- a click on the backdrop keeps the focus in the field -->
<div
  class="backdrop"
  role="presentation"
  onmousedown={(e) => e.target === e.currentTarget && e.preventDefault()}
  {onkeydown}
>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="add-title">
    <h2 id="add-title">{forId ? "Add PDF" : "Add paper"}{#if paper}<span class="of"> to {paper.key}</span>{/if}</h2>

    <div class="row">
      <span class="label">PDF</span>
      {#if pdf}
        <span class="file" title={pdf.path}>{file}{#if folder}<span class="folder">{folder}</span>{/if}</span>
      {:else}
        <span class="file muted">none yet</span>
      {/if}
      <span class="spacer"></span>
      {#if article && !pdf}
        {#if article.free === undefined}
          <span class="muted">looking for a free PDF…</span>
        {:else if article.free}
          <button disabled={!!busy} onclick={() => download(article!.free!, ++seq)}>Download free PDF</button>
        {/if}
        <button onclick={() => openUrl(article!.page)} title={article.page}>Open in browser</button>
      {/if}
      {#if pdf && !forId}
        <button disabled={!!busy} onclick={again} title="The DOI the PDF gives, and its entry">Read PDF again</button>
      {/if}
      <button disabled={!!busy} onclick={chooseFile}>{pdf ? "Change…" : "Choose…"}</button>
    </div>

    <div class="row">
      <span class="label">DOI</span>
      <input
        bind:this={input}
        bind:value={field}
        onpaste={paste}
        type="text"
        placeholder="DOI, doi.org link, article link or PDF path"
        spellcheck="false"
        autocomplete="off"
      />
      <button disabled={!!busy || field.trim() === ""} onclick={go}>Fetch</button>
      {#if forId}
        <select bind:value={merging} title="How the fetched entry is merged into the paper's">
          <option value="keep" title="Add the fields the paper lacks; where they differ, keep the paper's">Fill in</option>
          <option value="update" title="Add the fields the paper lacks; where they differ, take the fetched values">Update</option>
        </select>
      {/if}
    </div>
    {#if sourceText}
      <p class="source" class:ok={source === "pdf" || source === "page"}>{sourceText}</p>
    {/if}

    {#if busy}
      <p class="said">{busy}</p>
    {:else if message}
      <p class="said" class:error={message.error}>{message.text}</p>
    {/if}

    <div class="entry">
      {#if entry}
        <p class="title">{entry.title}</p>
        {#if entry.authors.length}<p class="authors">{entry.authors.join(", ")}</p>{/if}
        {#if entry.container || entry.year}
          <p class="muted">{[entry.container, entry.year].filter(Boolean).join(" · ")}</p>
        {/if}
      {:else if checked && "error" in checked}
        <p class="error">{checked.error}</p>
      {:else if entries.length > 1}
        <p class="muted">{entries.length} entries</p>
      {:else}
        <p class="muted">The entry: from a PDF, a DOI or a link above, or BibTeX below</p>
      {/if}
      <button class="disclosure" onclick={() => (folded = open)} aria-expanded={open}>
        <span class="caret" class:open>▸</span> BibTeX
      </button>
      {#if open}
        <textarea
          bind:value={text}
          placeholder={"@article{…}"}
          spellcheck="false"
          autocomplete="off"
          autocapitalize="off"
          rows="10"
        ></textarea>
      {/if}
    </div>

    {#if known.length}
      <div class="known">
        {#each known as r (r.id)}
          <p>Already in the library: {[r.authors, r.year].filter(Boolean).join(" ")}, {r.title}</p>
        {/each}
      </div>
    {/if}

    {#if pdf}
      <div class="row">
        <span class="label">File</span>
        {#if native}
          <span class="muted">A plain .bib file: the paper links the file where it is</span>
        {:else}
          <span class="modes" role="group" aria-label="What is done with the file">
            {#each MODES as m (m.mode)}
              <button
                class:active={mode === m.mode}
                disabled={m.mode === "link" && pdf.downloaded}
                title={m.mode === "link" && pdf.downloaded ? "A file downloaded is always placed in the library" : m.title}
                onclick={() => {
                  mode = m.mode;
                  modeChosen = true;
                }}>{m.name}</button
              >
            {/each}
          </span>
          <span class="muted">{mode === "link" ? "linked where it is" : "named by the library's template"}</span>
        {/if}
      </div>
    {/if}

    {#if error}
      <p class="said error">{error}</p>
    {/if}
    <div class="actions">
      {#if bibs.length > 1 && !forId}
        <label class="target">
          To
          <select bind:value={target}>
            {#each bibs as { bib: b } (b)}
              <option value={b} title={b}>{name(b)}</option>
            {/each}
          </select>
        </label>
      {/if}
      {#if known.length && papers && !forId}
        <label class="target">
          Already there
          <select bind:value={onDuplicate}>
            <option value="skip" title="Leave the paper in the library as it is">Skip</option>
            <option value="keep" title="Add the fields it lacks; where they differ, keep the library's">Fill in</option>
            <option value="update" title="Add the fields it lacks; where they differ, take the new values">Update</option>
          </select>
        </label>
      {/if}
      <span class="spacer"></span>
      <button onclick={onclose}>Cancel</button>
      <button class="primary" disabled={!canAdd} title={label(ADD)} onclick={add}>
        {adding ? "Adding…" : known.length && !papers && !forId ? "Add anyway" : "Add"}
      </button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    /* under the dialog that says papers is missing */
    z-index: 9;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.25);
  }
  .dialog {
    width: min(640px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    overflow-y: auto;
    box-sizing: border-box;
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
  h2 .of {
    margin-left: 6px;
    color: var(--muted);
    font-weight: 400;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 26px;
    margin-top: 6px;
  }
  .label {
    flex: none;
    width: 34px;
    color: var(--muted);
    font-size: 12px;
  }
  .file {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .muted,
  .folder {
    color: var(--muted);
  }
  .folder {
    margin-left: 8px;
  }
  input[type="text"] {
    flex: 1;
    min-width: 0;
    box-sizing: border-box;
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--fg);
    font: inherit;
    outline: none;
    -webkit-user-select: text;
    user-select: text;
  }
  input[type="text"]:focus,
  textarea:focus {
    border-color: var(--accent);
  }
  .entry {
    margin-top: 10px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 7px;
  }
  .entry p {
    margin: 0 0 3px;
    line-height: 1.4;
  }
  .title {
    font-weight: 600;
  }
  .disclosure {
    margin-top: 4px;
    padding: 0;
    border: none;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .caret {
    display: inline-block;
    transition: transform 0.1s;
  }
  .caret.open {
    transform: rotate(90deg);
  }
  textarea {
    display: block;
    box-sizing: border-box;
    width: 100%;
    margin-top: 6px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: transparent;
    color: var(--fg);
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
    line-height: 1.45;
    outline: none;
    resize: vertical;
    -webkit-user-select: text;
    user-select: text;
  }
  p {
    margin: 8px 0 0;
    line-height: 1.45;
  }
  .source {
    margin: 2px 0 0 42px;
    color: var(--muted);
    font-size: 12px;
  }
  .source.ok {
    color: var(--fg);
  }
  .said {
    color: var(--muted);
  }
  .said.error,
  .error {
    color: var(--error);
  }
  .known p {
    margin-top: 4px;
    overflow-wrap: anywhere;
  }
  .modes {
    display: inline-flex;
  }
  .modes button {
    border-radius: 0;
    margin-left: -1px;
  }
  .modes button:first-child {
    border-radius: 6px 0 0 6px;
    margin-left: 0;
  }
  .modes button:last-child {
    border-radius: 0 6px 6px 0;
  }
  .modes button.active {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
    position: relative;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 14px;
  }
  .spacer {
    flex: 1;
  }
  .target {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    color: var(--muted);
    font-size: 12px;
  }
  select {
    min-width: 0;
    max-width: 260px;
    font: inherit;
    font-size: 12px;
    color: var(--fg);
    background: var(--menu-bg);
  }
  button {
    flex: none;
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  button.disclosure {
    border: none;
    padding: 0;
  }
  button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
