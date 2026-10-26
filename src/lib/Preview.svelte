<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import * as cite from "$lib/cite";
  import { cached, thumbnail } from "$lib/pdf";
  import Share from "$lib/Share.svelte";
  import type { Item } from "$lib/types";

  let {
    item,
    marked,
    onopen,
  }: {
    item: Item;
    /** for each author, whether the search matched it */
    marked: boolean[];
    onopen: () => void;
  } = $props();

  const PAGE_WIDTH = 170;
  // wait for the selection to settle before reading a PDF or calling papers
  const DELAY = 150;

  let image = $state<string | null>(null);
  let citation = $state<string | null>(null);
  let citationError = $state<string | null>(null);
  let style = $state("");
  let bibtex = $state<string | null>(null);
  let failed = $state(false);
  // the BibTeX stays open or closed, formatted or raw, between sessions
  let showBibtex = $state(stored("bibtex", "open"));
  let raw = $state(stored("bibtex-view", "raw"));

  function stored(name: string, value: string): boolean {
    try {
      return localStorage.getItem(name) === value;
    } catch {
      return false;
    }
  }

  function store(name: string, value: string) {
    try {
      localStorage.setItem(name, value);
    } catch {}
  }

  function toggleBibtex() {
    showBibtex = !showBibtex;
    store("bibtex", showBibtex ? "open" : "closed");
  }

  function setRaw(on: boolean) {
    raw = on;
    store("bibtex-view", on ? "raw" : "formatted");
  }

  let hasPdf = $derived(item.files.some((f) => f.toLowerCase().endsWith(".pdf")));
  let source = $derived([item.container, item.year].filter(Boolean).join(" · "));

  $effect(() => {
    const id = item.id;
    image = cached(id) ?? null;
    citation = cite.cached(id) ?? null;
    citationError = null;
    failed = false;
    let live = true;
    const timer = setTimeout(() => {
      if (hasPdf && !image) {
        thumbnail(id, PAGE_WIDTH)
          .then((url) => live && (image = url))
          .catch(() => live && (failed = true));
      }
      if (citation === null) {
        cite
          .citation(id)
          .then(async (html) => {
            style = await cite.styleName();
            if (live) citation = html;
          })
          .catch((err) => live && (citationError = String(err)));
      }
    }, DELAY);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  });

  // the BibTeX is in memory: no delay
  $effect(() => {
    if (!showBibtex) return;
    let live = true;
    invoke<string>("bibtex", { id: item.id, raw })
      .then((text) => live && (bibtex = text))
      .catch((err) => live && (bibtex = String(err)));
    return () => {
      live = false;
    };
  });

  const fileName = (path: string) => path.split(/[\\/]/).pop();

  /** Drag the citation out, formatted for the apps that take it so. */
  function dragCitation(e: DragEvent) {
    if (!citation || !e.dataTransfer) return;
    const { text, html } = cite.toCopy(citation);
    e.dataTransfer.setData("text/plain", text);
    e.dataTransfer.setData("text/html", html);
    e.dataTransfer.effectAllowed = "copy";
  }
</script>

<div class="preview">
  <div class="head">
    <button class="page" onclick={onopen} title="Open" style:width="{PAGE_WIDTH}px">
      {#if image}
        <img src={image} alt="First page" />
      {:else}
        <span>{hasPdf ? (failed ? "Cannot read the PDF" : "") : "No PDF"}</span>
      {/if}
    </button>
    <div class="meta">
      <h2>{item.title}</h2>
      {#if item.authors.length}
        <p class="authors">
          {#each item.authors as author, i}{#if i}{", "}{/if}{#if marked[i]}<b>{author}</b>{:else}{author}{/if}{/each}
        </p>
      {/if}
      {#if source}<p>{source}</p>{/if}
      {#if item.doi}<p class="doi">{item.doi}</p>{/if}
      {#if item.keywords.length}
        <p class="keywords">
          {#each item.keywords as keyword}<span>{keyword}</span>{/each}
        </p>
      {/if}
      {#if item.files.length > 1}<p class="muted">{item.files.length} files</p>{/if}
    </div>
  </div>
  <section>
    <div class="section-head">
      <h3>Citation{#if style}<span class="muted"> · {style}</span>{/if}</h3>
      <Share {item} {citation} />
    </div>
    <!-- citeproc-js escapes the fields of the entry -->
    {#if citationError}
      <div class="citation error">{citationError}</div>
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="citation" draggable={!!citation} ondragstart={dragCitation}>{@html citation ?? ""}</div>
    {/if}
  </section>
  <section>
    <div class="bibtex-head">
      <button class="disclosure" onclick={toggleBibtex} aria-expanded={showBibtex}>
        <span class="caret" class:open={showBibtex}>▸</span> BibTeX
      </button>
      {#if showBibtex}
        <span class="views">
          <button class:active={!raw} onclick={() => setRaw(false)}>formatted</button>
          <button class:active={raw} onclick={() => setRaw(true)} title="As written in the file"
            >raw</button
          >
        </span>
        <span class="file" title={item.bib}>{fileName(item.bib)}</span>
      {/if}
    </div>
    {#if showBibtex}<pre class="bibtex">{bibtex ?? ""}</pre>{/if}
  </section>
</div>

<style>
  .preview {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px 16px;
    overflow-y: auto;
    -webkit-user-select: text;
    user-select: text;
  }
  .head {
    display: flex;
    gap: 14px;
    align-items: flex-start;
  }
  .page {
    flex: none;
    aspect-ratio: 1 / 1.414;
    padding: 0;
    border: 1px solid var(--line);
    border-radius: 3px;
    background: #ffffff;
    overflow: hidden;
    cursor: pointer;
  }
  .page img {
    display: block;
    width: 100%;
  }
  .page span {
    color: #8a8a8e;
    font-size: 12px;
  }
  .meta {
    min-width: 0;
    font-size: 12px;
  }
  h2 {
    margin: 0 0 6px;
    font-size: 15px;
    font-weight: 600;
    line-height: 1.3;
  }
  p {
    margin: 0 0 5px;
    line-height: 1.35;
  }
  .authors {
    display: -webkit-box;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .doi,
  .muted {
    color: var(--muted);
  }
  .doi {
    word-break: break-all;
  }
  .keywords {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .keywords span {
    padding: 1px 6px;
    border: 1px solid var(--line);
    border-radius: 4px;
    font-size: 11px;
  }
  h3,
  .disclosure {
    margin: 0 0 5px;
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
    font: inherit;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  h3 .muted {
    font-weight: 400;
    text-transform: none;
    letter-spacing: 0;
  }
  .disclosure {
    cursor: pointer;
  }
  .section-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
    margin-bottom: 5px;
  }
  .section-head h3 {
    margin: 0;
  }
  .bibtex-head {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .bibtex-head .file {
    margin-left: auto;
    color: var(--muted);
    font-size: 11px;
  }
  .views button {
    padding: 0 5px;
    border: none;
    border-radius: 3px;
    background: none;
    color: var(--muted);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }
  .views button.active {
    background: var(--line);
    color: var(--fg);
  }
  .caret {
    display: inline-block;
    transition: transform 0.1s;
  }
  .caret.open {
    transform: rotate(90deg);
  }
  .citation {
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
  .citation.error {
    color: var(--error);
  }
  .bibtex {
    margin: 0;
    padding: 8px 10px;
    border-radius: 6px;
    background: var(--line);
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    font-size: 10.5px;
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-all;
  }
</style>
