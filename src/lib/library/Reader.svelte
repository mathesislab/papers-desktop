<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { getDocument, type PDFDocumentLoadingTask } from "pdfjs-dist";
  import { EventBus, PDFFindController, PDFLinkService, PDFViewer } from "pdfjs-dist/web/pdf_viewer.mjs";
  import "pdfjs-dist/web/pdf_viewer.css";
  import { onMount, type Snippet } from "svelte";
  import "$lib/pdf"; // the worker of pdf.js

  type Entry = { title: string; dest: string | unknown[] | null; items: Entry[] };

  let {
    id,
    page,
    onpage,
    children,
  }: {
    id: string;
    /** the page to open at */
    page: number;
    onpage: (page: number) => void;
    /** at the end of the bar */
    children?: Snippet;
  } = $props();

  let container = $state<HTMLDivElement>();
  let pagesEl = $state<HTMLDivElement>();
  let findInput = $state<HTMLInputElement>();
  let count = $state(0);
  let current = $state(1);
  let error = $state<string | null>(null);
  let find = $state("");
  let outline = $state<Entry[]>([]);
  let showOutline = $state(false);

  let viewer: PDFViewer;
  let links: PDFLinkService;
  let bus: EventBus;
  let task: PDFDocumentLoadingTask | null = null;
  /** the pages fit the width, until zoomed */
  let fit = true;

  onMount(() => {
    bus = new EventBus();
    links = new PDFLinkService({ eventBus: bus });
    const finder = new PDFFindController({ eventBus: bus, linkService: links });
    viewer = new PDFViewer({
      container: container!,
      viewer: pagesEl,
      eventBus: bus,
      linkService: links,
      findController: finder,
    });
    links.setViewer(viewer);
    bus.on("pagesinit", () => {
      viewer.currentScaleValue = "page-width";
      if (page > 1) viewer.currentPageNumber = page;
    });
    bus.on("pagechanging", ({ pageNumber }: { pageNumber: number }) => {
      current = pageNumber;
      onpage(pageNumber);
    });
    const resize = new ResizeObserver(() => {
      if (fit && count) viewer.currentScaleValue = "page-width";
    });
    resize.observe(container!);
    load();
    return () => {
      resize.disconnect();
      task?.destroy();
    };
  });

  async function load() {
    try {
      const data = await invoke<ArrayBuffer>("pdf", { id });
      task = getDocument({ data: new Uint8Array(data) });
      const doc = await task.promise;
      count = doc.numPages;
      viewer.setDocument(doc);
      links.setDocument(doc);
      outline = ((await doc.getOutline()) ?? []) as Entry[];
    } catch (err) {
      error = String(err);
    }
  }

  export function focusFind() {
    findInput?.select();
  }

  /** In (1) or out (-1), or the width of the pane (0). */
  export function zoom(step: number) {
    if (!count) return;
    fit = step === 0;
    if (step > 0) viewer.increaseScale();
    else if (step < 0) viewer.decreaseScale();
    else viewer.currentScaleValue = "page-width";
  }

  function search(again: boolean, previous = false) {
    bus.dispatch("find", {
      source: null,
      type: again ? "again" : "",
      query: find,
      caseSensitive: false,
      entireWord: false,
      highlightAll: true,
      findPrevious: previous,
      matchDiacritics: false,
    });
  }

  function onfindkey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      search(true, e.shiftKey);
    } else if (e.key === "Escape") {
      e.preventDefault();
      find = "";
      bus.dispatch("findbarclose", { source: null });
      findInput?.blur();
    }
  }

  function go(entry: Entry) {
    showOutline = false;
    if (entry.dest) links.goToDestination(entry.dest);
  }

  /** The links of a PDF to the web open in the browser, not in the window. */
  function onclick(e: MouseEvent) {
    const link = (e.target as HTMLElement).closest("a");
    if (link?.href && /^(https?|mailto):/.test(link.href)) {
      e.preventDefault();
      e.stopPropagation();
      openUrl(link.href);
    }
  }
</script>

{#snippet entries(list: Entry[])}
  <ul>
    {#each list as entry}
      <li>
        <button onclick={() => go(entry)}>{entry.title}</button>
        {#if entry.items.length}{@render entries(entry.items)}{/if}
      </li>
    {/each}
  </ul>
{/snippet}

<div class="reader">
  <div class="bar">
    {#if outline.length}
      <button class:on={showOutline} onclick={() => (showOutline = !showOutline)}>Outline</button>
    {/if}
    <span class="pages">{count ? `Page ${current} of ${count}` : ""}</span>
    <button title="Zoom out" onclick={() => zoom(-1)}>−</button>
    <button title="Fit the width" onclick={() => zoom(0)}>Fit</button>
    <button title="Zoom in" onclick={() => zoom(1)}>+</button>
    <input
      bind:this={findInput}
      bind:value={find}
      type="search"
      placeholder="Find in the PDF"
      spellcheck="false"
      oninput={() => search(false)}
      onkeydown={onfindkey}
    />
    <button title="Open in the default PDF app" onclick={() => invoke("open", { id })}>Open ↗</button>
    {@render children?.()}
  </div>
  <div class="area">
    {#if showOutline}
      <nav class="outline">{@render entries(outline)}</nav>
    {/if}
    {#if error}
      <p class="error">{error}</p>
    {/if}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="container" bind:this={container} onclickcapture={onclick}>
      <div class="pdfViewer" bind:this={pagesEl}></div>
    </div>
  </div>
</div>

<style>
  .reader {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 46px;
    padding: 0 12px;
    border-bottom: 1px solid var(--line);
  }
  .bar button {
    padding: 3px 9px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
  }
  .bar button.on {
    background: var(--line);
  }
  .pages {
    flex: 1;
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  input {
    width: 180px;
    height: 26px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
    outline: none;
  }
  input:focus {
    border-color: var(--accent);
  }
  .area {
    flex: 1;
    position: relative;
    background: var(--line);
  }
  .container {
    position: absolute;
    inset: 0;
    overflow: auto;
    -webkit-user-select: text;
    user-select: text;
  }
  .outline {
    position: absolute;
    top: 8px;
    left: 8px;
    bottom: 8px;
    z-index: 2;
    width: 280px;
    overflow-y: auto;
    padding: 6px 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--menu-bg);
  }
  .outline ul {
    margin: 0;
    padding: 0 0 0 10px;
    list-style: none;
  }
  .outline button {
    display: block;
    width: 100%;
    padding: 3px 8px;
    border: none;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
    text-align: left;
  }
  .outline button:hover {
    background: var(--line);
  }
  .error {
    position: absolute;
    inset: 0;
    z-index: 1;
    margin: auto;
    height: fit-content;
    color: var(--error);
    text-align: center;
  }
</style>
