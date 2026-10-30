<script lang="ts">
  // Add a paper: from a PDF (its DOI is read, then its entry fetched), a DOI, the link of a
  // journal article, or BibTeX; with the PDF, which is moved, copied or left where it is.
  // The same sheet adds a PDF to a paper of the library (`forId`): the entry is that paper's.
  // The entry's BibTeX is in a field to fix before adding, with the key the library gives it.
  // An entry that is a paper of the .bib already is not added: the sheet compares it with the
  // paper, field by field, to merge into it, and the PDF is attached to it.

  import { invoke } from "@tauri-apps/api/core";
  import { downloadDir } from "@tauri-apps/api/path";
  import { open as pickFile } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount, untrack } from "svelte";
  import { holdModal } from "./modal.svelte";
  import { label } from "$lib/keys";
  import { origin } from "$lib/library/freepdf";
  import { defaultFiles, writers } from "$lib/library/writes";
  import type {
    AddOptions,
    Comparison,
    FilesMode,
    FreePdf,
    Incoming,
    Input,
    Item,
    Page,
    PdfRead,
    Row,
    Writer,
  } from "$lib/types";

  let {
    bib: preferred = null,
    rows,
    forId = null,
    pdf: first = null,
    choose: choosing = false,
    onadd,
    onchange,
    onshow,
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
    /** change the paper: its entry to `text` when that is not null, then the PDF added when that is not null */
    onchange: (
      id: string,
      text: string | null,
      pdf: string | null,
      files: FilesMode | null,
      fail: (message: string) => void,
    ) => Promise<boolean>;
    /** show a paper of the library: the sheet closes */
    onshow: (id: string) => void;
    onclose: () => void;
  } = $props();

  type Side = "library" | "new";
  const SIDES: Side[] = ["library", "new"];
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
  /** the PDF, whether the app downloaded it (papers always places a download in the library), and what a free PDF
   *  found is and where it came from */
  let pdf = $state<{ path: string; downloaded: boolean; origin?: string } | null>(null);
  /** where the DOI in the field came from */
  let source = $state<"pdf" | "search" | "typed" | "page" | "entry" | "none" | null>(null);
  /** the article the entry is of, the link to open in a browser; whether its free PDF is being looked for */
  let article = $state<{ page: string; looking: boolean } | null>(null);
  /** the .bib files of the library, and their writers; the one to add to */
  let bibs = $state<Writer[]>([]);
  let target = $state("");
  /** for a paper that has an entry already: how an entry fetched is merged into it */
  let merging = $state<"keep" | "update">("keep");
  /** what is done with the PDF */
  let mode = $state<FilesMode>("move");
  let modeChosen = false;
  /** the key the library gave the entry fetched (its key template, as `--update-key` makes it), which the BibTeX
   *  has until the user changes it */
  let made = $state<string | null>(null);
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
  /** a download failed: the article is to open in the browser */
  let failed = $state(false);
  /** the user says the entry is another paper than the one of the .bib it looks the same as (a plain .bib only) */
  let separate = $state(false);
  /** the entry against the paper of the library, and what is taken of each field that differs */
  let comparison = $state<Comparison | null>(null);
  let compareError = $state<string | null>(null);
  let choices = $state<Record<string, Side>>({});
  /** whether the PDF is attached to the paper that has the entry already */
  let attach = $state(true);
  /** the latest thing the user started: what an older one finds out is dropped */
  let seq = 0;

  let entries = $derived(checked && "entries" in checked ? checked.entries : []);
  /** the papers of the .bib to add to that each entry is the same as (the paper that gets the PDF is none) */
  let sames = $derived(
    entries.map((e) => e.same.filter((id) => id !== forId && rows.get(id)?.bib === target)),
  );
  /** every entry is a paper of the .bib already: the first is compared with it, the others are kept as they are */
  let duplicate = $derived(forId === null && !separate && entries.length > 0 && sames.every((ids) => ids.length > 0));
  let sameId = $derived(duplicate ? sames[0][0] : null);
  let same = $derived(sameId ? rows.get(sameId) : undefined);
  /** the papers of the library the entries are the same as, and that are not compared: those that the .bib has
   *  (when some entries are new), and those of another .bib */
  let known = $derived.by(() => {
    const found: { row: Row; here: boolean }[] = [];
    entries.forEach((e, i) => {
      for (const id of e.same) {
        const row = rows.get(id);
        const here = sames[i].includes(id);
        if (row && id !== forId && !(here && duplicate) && !found.some((k) => k.row.id === id)) found.push({ row, here });
      }
    });
    return found;
  });
  let kind = $derived(bibs.find((w) => w.bib === target)?.kind);
  /** whether papers writes the .bib file (papers-rs or a papers command): it merges the papers already there, makes keys, places files */
  let papers = $derived(kind === "papers-rs" || kind === "papers");
  let native = $derived(kind === "native");
  /** the fields to take from the entry: those with the new value chosen */
  let taken = $derived(comparison ? comparison.differences.filter((d) => (choices[d.field] ?? d.default) === "new") : []);
  let attaching = $derived(pdf !== null && attach);
  let canAdd = $derived(
    duplicate
      ? comparison !== null && !adding && !busy && (taken.length > 0 || attaching)
      : entries.length > 0 && !adding && !busy && target !== "" && (forId === null || pdf !== null),
  );
  /** what the PDF becomes in the library */
  let placed = $derived(native || mode === "link" ? "linked where it is" : mode === "move" ? "moved" : "copied");
  /** what the main button does, and what it says of it */
  let action = $derived.by(() => {
    if (adding) return { label: "Working…", summary: "" };
    if (forId) return { label: "Add PDF", summary: "" };
    if (duplicate) {
      const key = same?.key ?? "";
      const n = taken.length;
      if (n) {
        const parts = [`${n} ${n === 1 ? "field" : "fields"}`];
        if (attaching) parts.push("attach PDF");
        return { label: `Merge into ${key}`, summary: parts.join(", ") };
      }
      if (attaching) return { label: `Attach PDF to ${key}`, summary: `PDF ${placed}` };
      return comparison ? { label: "Nothing to change", summary: "" } : { label: `Merge into ${key}`, summary: "" };
    }
    const anyway = native && known.some((k) => k.here);
    const label = anyway ? "Add anyway" : entries.length > 1 ? `Add ${entries.length} papers` : "Add paper";
    return { label, summary: pdf ? `PDF ${placed}` : "" };
  });
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

  onMount(holdModal);

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

  // the entry against the paper of the library that is the same, a moment after typing; what was chosen
  // of a field stays, and a PDF is attached to a paper that has none
  let compared: { id: string; files: boolean } | null = null;
  $effect(() => {
    const id = sameId;
    const bibtex = text;
    if (!id) {
      comparison = null;
      compareError = null;
      compared = null;
      return;
    }
    let live = true;
    const timer = setTimeout(async () => {
      try {
        const found = await invoke<Comparison>("compare_entry", { id, text: bibtex });
        if (!live) return;
        const fresh = compared?.id !== id;
        const next: Record<string, Side> = {};
        for (const d of found.differences) next[d.field] = (!fresh && choices[d.field]) || d.default;
        choices = next;
        if (fresh || compared?.files !== found.files) attach = !found.files;
        compared = { id, files: found.files };
        comparison = found;
        compareError = null;
      } catch (e) {
        if (live) compareError = String(e);
      }
    }, 100);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  });

  // another .bib to add to has its own key template and its own keys: the key made follows
  $effect(() => {
    const bib = target;
    untrack(() => {
      const current = text;
      if (!bib || made === null || keyOf(current) !== made) return;
      libraryKey(current, bib).then((key) => {
        if (key && bib === target && text === current) {
          text = withKey(current, key);
          made = key;
        }
      });
    });
  });

  const fail = (e: unknown) => (message = { text: String(e), error: true });
  const keyOf = (bibtex: string) => /^\s*@\w+\s*\{\s*([^,\s]+)/.exec(bibtex)?.[1] ?? null;
  const withKey = (bibtex: string, key: string) => bibtex.replace(/^(\s*@\w+\s*\{\s*)[^,\s]+/, (_, head) => head + key);

  /** The key the library gives the entry: its key template, as `--update-key` makes it. None for a paper that
   *  has its entry, or when the library cannot say (the BibTeX shows the key it has). */
  async function libraryKey(bibtex: string, bib: string): Promise<string | null> {
    if (forId || !bib) return null;
    try {
      return await invoke<string>("library_key", { bib, text: bibtex });
    } catch {
      return null;
    }
  }

  /** An entry fetched is the BibTeX of the sheet, with the key the library gives it in place of the one it came with
   *  (the DOI, for Crossref): what is shown is what is written, and the user's change of the key stays. */
  async function use(fetched: string, n: number): Promise<boolean> {
    const key = await libraryKey(fetched, target);
    if (n !== seq) return false;
    text = key ? withKey(fetched, key) : fetched;
    made = key;
    return true;
  }

  /** Use a PDF, as the one to add: dropped, chosen, written in the field, downloaded. */
  export async function setPdf(path: string, downloaded = false) {
    const n = ++seq;
    pdf = { path, downloaded };
    error = null;
    message = null;
    failed = false;
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
        if (!(await use(found.bibtex, n))) return;
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
      } else if (!(await use(fetched, n))) {
        return false;
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

  /** The free PDF of the article, as `papers fetch --pdf` finds it, downloaded into a folder of the temporary files
   *  and made the PDF of the sheet; else the link to open in the browser (the published version's first), the
   *  highlighted button when the downloads were refused. */
  async function lookup(doi: string, page: string, n: number) {
    if (pdf) return;
    article = { page, looking: true };
    try {
      const found = await invoke<FreePdf>("fetch_free_pdf", { doi });
      if (n !== seq) return;
      if (found.kind === "found") {
        article = { page, looking: false };
        return await usePdf(found.path, n, origin(found));
      }
      article = { page: found.links[0] ?? page, looking: false };
      if (found.refused) {
        failed = true;
        message = { text: `${found.refused}: open it in the browser`, error: true };
      }
    } catch {
      if (n === seq && article) article = { page, looking: false };
    }
  }

  /** The PDF of a link, downloaded into a folder of the temporary files, and made the PDF of the sheet. */
  async function download(url: string, n: number) {
    busy = "Downloading the PDF…";
    message = null;
    failed = false;
    try {
      const path = await invoke<string>("download_pdf", { url });
      if (n !== seq) return;
      busy = null;
      await usePdf(path, n);
    } catch (e) {
      if (n === seq) {
        fail(e);
        // the browser may get what the app cannot: the link is made to open
        failed = true;
        article ??= { page: url, looking: false };
      }
    } finally {
      if (n === seq) busy = null;
    }
  }

  async function usePdf(path: string, n: number, origin?: string) {
    pdf = { path, downloaded: true, origin };
    failed = false;
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
        article = { page: page.url, looking: false };
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
    failed = false;
    switch (found.kind) {
      case "empty":
        message = null;
        return;
      case "bibtex":
        text = field;
        field = "";
        source = null;
        doiKnown = false;
        made = null;
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
    const fail = (m: string) => (error = m);
    try {
      if (duplicate) {
        // the paper of the library, with the values chosen, and the PDF as chosen
        const fields = taken.map((d) => d.field);
        const merged = fields.length ? await invoke<string>("merge_entry", { id: sameId, text, take: fields }) : null;
        await onchange(sameId!, merged, attaching ? pdf!.path : null, attaching && !native ? mode : null, fail);
      } else {
        const files = pdf && !native ? mode : null;
        if (forId) await onchange(forId, text, pdf!.path, files, fail);
        else await onadd(target, text, { onDuplicate: "skip", updateKey: false, pdf: pdf?.path ?? null, files }, fail);
      }
    } catch (e) {
      error = String(e);
    }
    adding = false;
  }

  /** Every field that differs takes the value of the library, or of the entry. */
  function pickAll(side: Side) {
    for (const d of comparison?.differences ?? []) choices[d.field] = side;
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
    <h2 id="add-title">
      {#if forId}
        Add PDF{#if paper}<span class="of"> to {paper.key}</span>{/if}
      {:else if duplicate && same}
        Same paper as
        <button class="link" title="Show it in the library" onclick={() => onshow(same.id)}>{same.key}</button>
        {#if bibs.length > 1}<span class="of">in {name(same.bib)}</span>{/if}
      {:else}
        Add paper
      {/if}
    </h2>

    <div class="row">
      <span class="label">PDF</span>
      {#if pdf}
        <span class="file" title={pdf.path}>{file}{#if folder}<span class="folder">{folder}</span>{/if}</span>
      {:else}
        <span class="file muted">none yet</span>
      {/if}
      <span class="spacer"></span>
      {#if article && !pdf}
        {#if article.looking}
          <span class="muted">looking for a free PDF…</span>
        {/if}
        <button class:primary={failed} onclick={() => openUrl(article!.page)} title={article.page}>Open in browser</button>
      {/if}
      {#if pdf && !forId}
        <button disabled={!!busy} onclick={again} title="The DOI the PDF gives, and its entry">Read PDF again</button>
      {/if}
      <button disabled={!!busy} onclick={chooseFile}>{pdf ? "Change…" : "Choose…"}</button>
    </div>
    {#if pdf?.origin}
      <p class="source">{pdf.origin}</p>
    {/if}

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

    {#if duplicate}
      <div class="compare">
        {#if compareError}
          <p class="error">{compareError}</p>
        {:else if comparison}
          {#if comparison.differences.length}
            <div class="head">
              <span class="muted">What differs: the value kept</span>
              <span class="spacer"></span>
              <span class="modes" role="group" aria-label="Keep the values of">
                <button class:active={taken.length === 0} onclick={() => pickAll("library")}>Keep library</button>
                <button class:active={taken.length === comparison.differences.length} onclick={() => pickAll("new")}>
                  Take all new
                </button>
              </span>
            </div>
            <table>
              <thead>
                <tr><th></th><th>In the library</th><th>New</th></tr>
              </thead>
              <tbody>
                {#each comparison.differences as d (d.field)}
                  {@const chosen = choices[d.field] ?? d.default}
                  <tr>
                    <th scope="row">{d.field === "ENTRYTYPE" ? "type" : d.field}</th>
                    {#each SIDES as side (side)}
                      {@const value = side === "library" ? d.library : d.new}
                      <td class:chosen={chosen === side}>
                        <label>
                          <input
                            type="radio"
                            name="keep-{d.field}"
                            checked={chosen === side}
                            onchange={() => (choices[d.field] = side)}
                          />
                          <span class="value" class:none={value === null} title={value ?? ""}>{value ?? "none"}</span>
                        </label>
                      </td>
                    {/each}
                  </tr>
                {/each}
              </tbody>
            </table>
          {:else}
            <p class="muted">Nothing differs: the paper has all of this entry</p>
          {/if}
          {#if pdf}
            <div class="row attach">
              {#if comparison.files}
                <span class="muted">The paper has a PDF</span>
                <label><input type="radio" name="attach" checked={attach} onchange={() => (attach = true)} /> Also attach</label>
                <label><input type="radio" name="attach" checked={!attach} onchange={() => (attach = false)} /> Don't attach</label>
              {:else}
                <label><input type="checkbox" bind:checked={attach} /> Attach this PDF</label>
                <span class="muted">The paper has no PDF yet</span>
              {/if}
            </div>
          {/if}
          {#if entries.length > 1}
            <p class="muted">
              {entries.length - 1} more already in the library: kept as {entries.length === 2 ? "it is" : "they are"}
            </p>
          {/if}
          {#if native}
            <p class="muted">
              Another paper? <button class="link" onclick={() => (separate = true)}>Add it as a new paper</button>
            </p>
          {/if}
        {:else}
          <p class="muted">Comparing…</p>
        {/if}
      </div>
    {/if}

    {#if known.length}
      <div class="known">
        {#each known as { row: r, here } (r.id)}
          <p>
            {here ? "Already in the library" : `Also in ${name(r.bib)}`}: {[r.authors, r.year].filter(Boolean).join(" ")}, {r.title}{here && papers
              ? " (kept as it is)"
              : ""}
          </p>
        {/each}
      </div>
    {/if}

    {#if pdf && (forId || !duplicate || attaching)}
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
      <span class="spacer"></span>
      <button onclick={onclose}>Cancel</button>
      {#if action.summary}<span class="summary" title={action.summary}>{action.summary}</span>{/if}
      <button class="primary" disabled={!canAdd} title={label(ADD)} onclick={add}>{action.label}</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    /* the sheet opens near the top of the window and grows downwards, never up */
    --top: max(24px, 9vh);
    position: fixed;
    inset: 0;
    /* under the dialog that says papers is missing */
    z-index: 9;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    box-sizing: border-box;
    padding: var(--top) 16px 16px;
    background: rgba(0, 0, 0, 0.25);
  }
  .dialog {
    width: min(640px, 100%);
    max-height: 100%;
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
    margin-left: 2px;
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
  /* what says what went wrong has the link, which is copied from here */
  .said,
  .error,
  .known p {
    -webkit-user-select: text;
    user-select: text;
    overflow-wrap: anywhere;
  }
  .known p {
    margin-top: 4px;
  }
  .compare {
    margin-top: 10px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 7px;
  }
  .compare p {
    margin: 4px 0 0;
  }
  .compare .head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
    font-size: 12px;
  }
  th {
    padding: 3px 6px;
    color: var(--muted);
    font-weight: 400;
    text-align: left;
  }
  thead th {
    font-size: 11px;
  }
  tbody th {
    width: 64px;
    vertical-align: top;
    overflow-wrap: anywhere;
  }
  td {
    padding: 0;
    vertical-align: top;
    border-top: 1px solid var(--border);
  }
  tbody th {
    border-top: 1px solid var(--border);
  }
  td label {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    padding: 4px 6px;
    cursor: pointer;
  }
  td.chosen {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  td input,
  .attach input {
    flex: none;
    margin: 2px 0 0;
    accent-color: var(--accent);
  }
  .value {
    min-width: 0;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
    overflow-wrap: anywhere;
    line-height: 1.4;
  }
  .value.none {
    color: var(--muted);
    font-style: italic;
  }
  .attach {
    gap: 14px;
    margin-top: 8px;
  }
  .attach label {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .attach input {
    margin: 0;
  }
  .summary {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
    font-size: 12px;
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
  button.link {
    padding: 0;
    border: none;
    color: var(--accent);
    font-size: inherit;
    font-weight: inherit;
    text-decoration: underline;
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
