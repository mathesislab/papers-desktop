<script lang="ts">
  // Add papers: a DOI (or its doi.org link) is fetched, BibTeX is used as it
  // is. Either way the BibTeX is in the field to fix before adding, and
  // the papers of the library that are the same are listed under it.

  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { label } from "$lib/keys";
  import { writers } from "$lib/library/writes";
  import type { Incoming, Row } from "$lib/types";

  let {
    bib: preferred = null,
    rows,
    onadd,
    onclose,
  }: {
    /** the .bib file to add to, unless the user chooses another: that of the paper selected */
    bib?: string | null;
    /** the papers of the library, by id */
    rows: Map<string, Row>;
    /** write the BibTeX; whether it was written, else `fail` was told why */
    onadd: (bib: string, text: string, fail: (message: string) => void) => Promise<boolean>;
    onclose: () => void;
  } = $props();

  /** a DOI, with or without its doi.org link or `doi:` (as fetch.rs reads it) */
  const DOI = /^(?:https?:\/\/(?:dx\.)?doi\.org\/|doi:\s*)?10\.\d{4,9}\/\S+$/i;
  const ADD = { key: "Enter", mod: true };

  let text = $state("");
  let field = $state<HTMLTextAreaElement>();
  /** the .bib files of the library; the one to add to */
  let bibs = $state<string[]>([]);
  let target = $state("");
  /** the DOI being fetched, and the one that failed with why */
  let fetching = $state<string | null>(null);
  let failed = $state<{ doi: string; message: string } | null>(null);
  /** the entries of the BibTeX, or why it does not parse */
  let checked = $state<{ entries: Incoming[] } | { error: string } | null>(null);
  let adding = $state(false);
  let error = $state<string | null>(null);

  let trimmed = $derived(text.trim());
  let kind = $derived(trimmed === "" ? "empty" : trimmed.startsWith("@") ? "bibtex" : DOI.test(trimmed) ? "doi" : "other");
  let entries = $derived(checked && "entries" in checked ? checked.entries : []);
  /** the papers of the library the entries are the same as */
  let known = $derived.by(() => {
    const ids = [...new Set(entries.flatMap((e) => e.same))];
    return ids.map((id) => rows.get(id)).filter((r): r is Row => r !== undefined);
  });
  let canAdd = $derived(kind === "bibtex" && entries.length > 0 && !adding && target !== "");

  /** the line under the field: what it is taken for */
  let said = $derived.by((): { text: string; error?: boolean } => {
    if (kind === "empty") return { text: "A DOI, a doi.org link or BibTeX" };
    if (kind === "other") return { text: "Not a DOI or BibTeX", error: true };
    if (kind === "doi") {
      if (fetching === trimmed) return { text: "DOI: fetching…" };
      if (failed?.doi === trimmed) return { text: failed.message, error: true };
      return { text: "DOI: Enter fetches its BibTeX" };
    }
    if (!checked) return { text: "BibTeX: checking…" };
    if ("error" in checked) return { text: checked.error, error: true };
    const n = checked.entries.length;
    return { text: `BibTeX: ${n} ${n === 1 ? "entry" : "entries"}` };
  });

  onMount(async () => {
    field?.focus();
    bibs = (await writers()).map((w) => w.bib);
    target = preferred && bibs.includes(preferred) ? preferred : (bibs[0] ?? "");
  });

  // the entries of the BibTeX, and the papers they are the same as: a moment
  // after typing, and again when the library is read again
  $effect(() => {
    const bibtex = kind === "bibtex" ? text : null;
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

  /** The BibTeX of the DOI in the field, in its place. */
  async function fetchDoi() {
    const doi = trimmed;
    fetching = doi;
    failed = null;
    try {
      const bibtex = await invoke<string>("fetch_doi", { doi });
      // not when the field changed meanwhile
      if (text.trim() === doi) text = bibtex;
    } catch (e) {
      failed = { doi, message: String(e) };
    } finally {
      if (fetching === doi) fetching = null;
    }
  }

  async function add() {
    if (!canAdd) return;
    adding = true;
    error = null;
    await onadd(target, text, (message) => (error = message));
    adding = false;
  }

  /** A DOI pasted is fetched at once, one typed on Enter. */
  function paste() {
    setTimeout(() => {
      if (kind === "doi" && !fetching) fetchDoi();
    });
  }

  function onkeydown(e: KeyboardEvent) {
    // the keys of the library (arrows, Enter, Escape) are not for the sheet
    e.stopPropagation();
    const mod = e.metaKey || e.ctrlKey;
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "Enter" && (mod || kind === "doi")) {
      e.preventDefault();
      if (kind === "doi") fetchDoi();
      else add();
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
    <h2 id="add-title">Add papers</h2>
    <textarea
      bind:this={field}
      bind:value={text}
      onpaste={paste}
      placeholder={"A DOI (10.1038/171737a0), its doi.org link, or BibTeX (@article{…})"}
      spellcheck="false"
      autocomplete="off"
      rows="12"
    ></textarea>
    <p class="said" class:error={said.error}>{said.text}</p>
    {#if known.length}
      <div class="known">
        {#each known as r (r.id)}
          <p>Already in the library: {[r.authors, r.year].filter(Boolean).join(" ")}, {r.title}</p>
        {/each}
      </div>
    {/if}
    {#if error}
      <p class="said error">{error}</p>
    {/if}
    <div class="actions">
      {#if bibs.length > 1}
        <label class="target">
          To
          <select bind:value={target}>
            {#each bibs as b (b)}
              <option value={b} title={b}>{name(b)}</option>
            {/each}
          </select>
        </label>
      {/if}
      <span class="spacer"></span>
      <button onclick={onclose}>Cancel</button>
      <button class="primary" disabled={!canAdd} title={label(ADD)} onclick={add}>
        {adding ? "Adding…" : known.length ? "Add anyway" : "Add"}
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
  textarea {
    display: block;
    box-sizing: border-box;
    width: 100%;
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
  textarea:focus {
    border-color: var(--accent);
  }
  p {
    margin: 8px 0 0;
    line-height: 1.45;
  }
  .said {
    color: var(--muted);
  }
  .said.error {
    color: var(--error);
  }
  .known p {
    margin-top: 4px;
    overflow-wrap: anywhere;
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
