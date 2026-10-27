// A PDF read in a paper tab: the pdf.js viewer, and what the toolbar of the
// window shows of it.

import { invoke } from "@tauri-apps/api/core";
import { getDocument, type PDFDocumentLoadingTask } from "pdfjs-dist";
import { EventBus, PDFFindController, PDFLinkService, PDFViewer } from "pdfjs-dist/web/pdf_viewer.mjs";
import "$lib/pdf"; // the worker of pdf.js

/** an entry of the outline of a PDF */
export type Entry = { title: string; dest: string | unknown[] | null; items: Entry[] };

export class PdfReader {
  readonly id: string;
  current = $state(1);
  count = $state(0);
  error = $state<string | null>(null);
  outline = $state<Entry[]>([]);
  showOutline = $state(false);
  find = $state("");
  /** the find field of the toolbar */
  findInput = $state<HTMLInputElement>();

  #page: number;
  #onpage: (page: number) => void;
  #viewer?: PDFViewer;
  #links?: PDFLinkService;
  #bus?: EventBus;
  #task: PDFDocumentLoadingTask | null = null;
  /** the pages fit the width, until zoomed */
  #fit = true;

  /** The PDF of item `id`, opened at `page`; `onpage` hears of the page in view. */
  constructor(id: string, page: number, onpage: (page: number) => void) {
    this.id = id;
    this.#page = page;
    this.#onpage = onpage;
  }

  /** Show the PDF in `container`, its pages in `pages`; returns what takes it away. */
  mount(container: HTMLDivElement, pages: HTMLDivElement): () => void {
    const bus = (this.#bus = new EventBus());
    const links = (this.#links = new PDFLinkService({ eventBus: bus }));
    const finder = new PDFFindController({ eventBus: bus, linkService: links });
    const viewer = (this.#viewer = new PDFViewer({
      container,
      viewer: pages,
      eventBus: bus,
      linkService: links,
      findController: finder,
    }));
    links.setViewer(viewer);
    bus.on("pagesinit", () => {
      viewer.currentScaleValue = "page-width";
      if (this.#page > 1) viewer.currentPageNumber = this.#page;
    });
    bus.on("pagechanging", ({ pageNumber }: { pageNumber: number }) => {
      this.current = pageNumber;
      this.#onpage(pageNumber);
    });
    const resize = new ResizeObserver(() => {
      if (this.#fit && this.count) viewer.currentScaleValue = "page-width";
    });
    resize.observe(container);
    this.#load(viewer, links);
    return () => {
      resize.disconnect();
      this.#task?.destroy();
    };
  }

  async #load(viewer: PDFViewer, links: PDFLinkService) {
    try {
      const data = await invoke<ArrayBuffer>("pdf", { id: this.id });
      this.#task = getDocument({ data: new Uint8Array(data) });
      const doc = await this.#task.promise;
      this.count = doc.numPages;
      viewer.setDocument(doc);
      links.setDocument(doc);
      this.outline = ((await doc.getOutline()) ?? []) as Entry[];
    } catch (err) {
      this.error = String(err);
    }
  }

  focusFind() {
    this.findInput?.select();
  }

  /** In (1) or out (-1), or the width of the pane (0). */
  zoom(step: number) {
    const viewer = this.#viewer;
    if (!viewer || !this.count) return;
    this.#fit = step === 0;
    if (step > 0) viewer.increaseScale();
    else if (step < 0) viewer.decreaseScale();
    else viewer.currentScaleValue = "page-width";
  }

  /** Find the text of the find field: from the start, or `again` (the next or previous). */
  search(again: boolean, previous = false) {
    this.#bus?.dispatch("find", {
      source: null,
      type: again ? "again" : "",
      query: this.find,
      caseSensitive: false,
      entireWord: false,
      highlightAll: true,
      findPrevious: previous,
      matchDiacritics: false,
    });
  }

  closeFind() {
    this.find = "";
    this.#bus?.dispatch("findbarclose", { source: null });
    this.findInput?.blur();
  }

  go(entry: Entry) {
    this.showOutline = false;
    if (entry.dest) this.#links?.goToDestination(entry.dest);
  }
}
