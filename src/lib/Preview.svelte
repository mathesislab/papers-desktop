<script lang="ts" module>
  const bibtexCache = new Map<string, string>();

  /** Forget the BibTeX entries: the library changed. */
  export function forgetBibtex() {
    bibtexCache.clear();
  }
</script>

<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import * as cite from "$lib/cite";
  import { cached, thumbnail } from "$lib/pdf";
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
  let style = $state("");
  let bibtex = $state<string | null>(null);
  let failed = $state(false);
  let showBibtex = $state(storedBibtex());

  // the BibTeX stays open or closed between sessions
  function storedBibtex(): boolean {
    try {
      return localStorage.getItem("bibtex") === "open";
    } catch {
      return false;
    }
  }

  function toggleBibtex() {
    showBibtex = !showBibtex;
    try {
      localStorage.setItem("bibtex", showBibtex ? "open" : "closed");
    } catch {}
  }

  let hasPdf = $derived(item.files.some((f) => f.toLowerCase().endsWith(".pdf")));
  let source = $derived([item.container, item.year].filter(Boolean).join(" · "));

  $effect(() => {
    const key = item.key;
    const withBibtex = showBibtex;
    image = cached(key) ?? null;
    citation = cite.cached(key) ?? null;
    bibtex = bibtexCache.get(key) ?? null;
    failed = false;
    let live = true;
    const timer = setTimeout(() => {
      if (hasPdf && !image) {
        thumbnail(key, PAGE_WIDTH)
          .then((url) => live && (image = url))
          .catch(() => live && (failed = true));
      }
      if (citation === null) {
        cite
          .citation(key)
          .then(async (html) => {
            style = await cite.styleName();
            if (live) citation = html;
          })
          .catch((err) => live && (citation = `<span class="error">${escape(String(err))}</span>`));
      }
      // from papers, only when shown
      if (withBibtex && bibtex === null) {
        invoke<string>("bibtex", { key })
          .then((text) => {
            bibtexCache.set(key, text);
            if (live) bibtex = text;
          })
          .catch((err) => live && (bibtex = String(err)));
      }
    }, DELAY);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  });

  function escape(text: string): string {
    return text.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]!);
  }
</script>

<aside class="preview">
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
          {#each item.authors as author, i}{#if i}, {/if}{#if marked[i]}<b>{author}</b>{:else}{author}{/if}{/each}
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
    <h3>Citation{#if style}<span class="muted"> · {style}</span>{/if}</h3>
    <!-- citeproc-js escapes the fields of the entry -->
    <div class="citation">{@html citation ?? ""}</div>
  </section>
  <section>
    <button class="disclosure" onclick={toggleBibtex} aria-expanded={showBibtex}>
      <span class="caret" class:open={showBibtex}>▸</span> BibTeX
    </button>
    {#if showBibtex}<pre class="bibtex">{bibtex ?? ""}</pre>{/if}
  </section>
</aside>

<style>
  .preview {
    flex: none;
    width: 500px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px 16px;
    border-left: 1px solid var(--line);
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
  .citation :global(.error) {
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
