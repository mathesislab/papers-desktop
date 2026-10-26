<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { cached, thumbnail } from "$lib/pdf";
  import type { Item } from "$lib/types";

  let { item, onopen }: { item: Item; onopen: () => void } = $props();

  const PAGE_WIDTH = 170;
  // wait for the selection to settle before reading a PDF or calling papers
  const DELAY = 150;
  const bibtexCache = new Map<string, string>();

  let image = $state<string | null>(null);
  let bibtex = $state<string | null>(null);
  let failed = $state(false);

  let hasPdf = $derived(item.files.some((f) => f.toLowerCase().endsWith(".pdf")));
  let source = $derived([item.container, item.year].filter(Boolean).join(" · "));

  $effect(() => {
    const key = item.key;
    image = cached(key) ?? null;
    bibtex = bibtexCache.get(key) ?? null;
    failed = false;
    let live = true;
    const timer = setTimeout(() => {
      if (hasPdf && !image) {
        thumbnail(key, PAGE_WIDTH)
          .then((url) => live && (image = url))
          .catch(() => live && (failed = true));
      }
      if (bibtex === null) {
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
      {#if item.authors.length}<p class="authors">{item.authors.join(", ")}</p>{/if}
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
  <pre class="bibtex">{bibtex ?? ""}</pre>
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
