// A PDF shown by PDFKit (macOS): a native view over the reader area, which
// this keeps in place, and drives through tauri-plugin-pdfkit (its path asked
// of the app, `pdf_path`).

import { invoke } from "@tauri-apps/api/core";
import * as pdfkit from "tauri-plugin-pdfkit-api";
import { Reader } from "./reader.svelte";

export class NativeReader extends Reader {
  /** the commands to the view, in order: a search typed letter by letter
   *  must not land after the next one */
  #queue: Promise<unknown> = Promise.resolve();

  #send<T>(command: () => Promise<T>): Promise<T> {
    const sent = this.#queue.then(command);
    this.#queue = sent.catch(() => {});
    return sent;
  }

  /** a modal of the page is open: the view is hidden, as it lies above the page */
  #covered = false;

  cover(covered: boolean) {
    this.#covered = covered;
    this.#send(() => pdfkit.cover(this.id, covered));
  }

  mount(area: HTMLDivElement): () => void {
    let opened = false;
    // the area changes with the panes, the outline, the window
    const resize = new ResizeObserver(() => {
      if (opened) this.#send(() => pdfkit.setFrame(pdfkit.frameOf(area)));
    });
    resize.observe(area);
    const unlisten = pdfkit.onPage((page) => this.turned(page));
    this.#send(async () => {
      const path = await invoke<string>("pdf_path", { id: this.id });
      return pdfkit.open(this.id, path, this.page, pdfkit.frameOf(area), this.#covered);
    })
      .then(({ count, outline }) => {
        opened = true;
        this.count = count;
        this.outline = outline;
        this.current = this.page;
      })
      .catch((err) => (this.error = String(err)));
    return () => {
      resize.disconnect();
      unlisten.then((f) => f());
      this.#send(() => pdfkit.close(this.id));
    };
  }

  zoom(step: number) {
    this.#send(() => pdfkit.zoom(step));
  }

  search(again: boolean, previous = false) {
    this.#send(() => pdfkit.find(this.find, again, previous));
  }

  protected clearFind() {
    this.#send(() => pdfkit.find("", false, false));
  }

  protected goTo(target: unknown) {
    this.#send(() => pdfkit.go(target as number[]));
  }
}
