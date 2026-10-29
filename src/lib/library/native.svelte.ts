// A PDF shown by PDFKit (macOS): a native view over the reader area, which
// this keeps in place, and drives through commands of src-tauri/src/pdf_view.rs.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Reader, type Entry } from "./reader.svelte";

/** mirrors pdf_view::Opened */
type Opened = { count: number; outline: Entry[] };

/** The place of an element in the page, and the height of the page, as the
 *  view takes them (mirrors pdf_view::Frame). */
function frame(el: HTMLElement) {
  const box = el.getBoundingClientRect();
  return { x: box.left, y: box.top, width: box.width, height: box.height, viewport: window.innerHeight };
}

export class NativeReader extends Reader {
  /** the commands to the view, in order: a search typed letter by letter
   *  must not land after the next one */
  #queue: Promise<unknown> = Promise.resolve();

  #send<T>(command: string, args: Record<string, unknown>): Promise<T> {
    const sent = this.#queue.then(() => invoke<T>(command, args));
    this.#queue = sent.catch(() => {});
    return sent;
  }

  /** a modal of the page is open: the view is hidden, as it lies above the page */
  #covered = false;

  cover(covered: boolean) {
    this.#covered = covered;
    this.#send("pdf_cover", { id: this.id, covered });
  }

  mount(area: HTMLDivElement): () => void {
    let opened = false;
    // the area changes with the panes, the outline, the window
    const resize = new ResizeObserver(() => {
      if (opened) this.#send("pdf_frame", { frame: frame(area) });
    });
    resize.observe(area);
    const unlisten = listen<number>("pdf-page", (e) => this.turned(e.payload));
    this.#send<Opened>("pdf_open", { id: this.id, page: this.page, frame: frame(area), covered: this.#covered })
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
      this.#send("pdf_close", { id: this.id });
    };
  }

  zoom(step: number) {
    this.#send("pdf_zoom", { step });
  }

  search(again: boolean, previous = false) {
    this.#send("pdf_find", { text: this.find, again, backwards: previous });
  }

  protected clearFind() {
    this.#send("pdf_find", { text: "", again: false, backwards: false });
  }

  protected goTo(target: unknown) {
    this.#send("pdf_go", { target });
  }
}
