// A PDF shown with pdf.js, where PDFKit is not (Linux).

import { invoke } from "@tauri-apps/api/core";
import { getDocument, type PDFDocumentLoadingTask } from "pdfjs-dist";
import { EventBus, PDFFindController, PDFLinkService, PDFViewer } from "pdfjs-dist/web/pdf_viewer.mjs";
import "pdfjs-dist/web/pdf_viewer.css";
import "$lib/pdf"; // the worker of pdf.js
import { Reader, type Entry } from "./reader.svelte";

/** an entry of the outline, as pdf.js gives it */
type PdfjsEntry = { title: string; dest: string | unknown[] | null; items: PdfjsEntry[] };

const entry = (e: PdfjsEntry): Entry => ({ title: e.title, target: e.dest, items: e.items.map(entry) });

export class PdfjsReader extends Reader {
  #viewer?: PDFViewer;
  #links?: PDFLinkService;
  #bus?: EventBus;
  #task: PDFDocumentLoadingTask | null = null;
  /** the pages fit the width, until zoomed */
  #fit = true;

  mount(area: HTMLDivElement): () => void {
    // pdf.js wants its container positioned, the pages in it
    const container = document.createElement("div");
    container.className = "pdfjs-container";
    const pages = document.createElement("div");
    pages.className = "pdfViewer";
    container.append(pages);
    area.append(container);

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
      if (this.page > 1) viewer.currentPageNumber = this.page;
    });
    bus.on("pagechanging", ({ pageNumber }: { pageNumber: number }) => this.turned(pageNumber));
    const resize = new ResizeObserver(() => {
      if (this.#fit && this.count) viewer.currentScaleValue = "page-width";
    });
    resize.observe(container);
    this.#load(viewer, links);
    return () => {
      resize.disconnect();
      this.#task?.destroy();
      container.remove();
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
      this.outline = (((await doc.getOutline()) ?? []) as PdfjsEntry[]).map(entry);
    } catch (err) {
      this.error = String(err);
    }
  }

  zoom(step: number) {
    const viewer = this.#viewer;
    if (!viewer || !this.count) return;
    this.#fit = step === 0;
    if (step > 0) viewer.increaseScale();
    else if (step < 0) viewer.decreaseScale();
    else viewer.currentScaleValue = "page-width";
  }

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

  protected clearFind() {
    this.#bus?.dispatch("findbarclose", { source: null });
  }

  protected goTo(target: unknown) {
    this.#links?.goToDestination(target as string | unknown[]);
  }
}
