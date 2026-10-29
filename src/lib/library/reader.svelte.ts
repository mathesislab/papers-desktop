// A PDF read in a paper tab, and what the toolbar of the window shows of it.
// macOS shows it with PDFKit (native.svelte.ts), elsewhere with pdf.js
// (pdfjs.svelte.ts).

/** an entry of the outline of a PDF; `target` is where it goes, as its reader knows it */
export type Entry = { title: string; target: unknown; items: Entry[] };

export abstract class Reader {
  readonly id: string;
  current = $state(1);
  count = $state(0);
  error = $state<string | null>(null);
  outline = $state<Entry[]>([]);
  showOutline = $state(false);
  find = $state("");
  /** the find field of the toolbar */
  findInput = $state<HTMLInputElement>();

  protected page: number;
  protected onpage: (page: number) => void;

  /** The PDF of item `id`, opened at `page`; `onpage` hears of the page in view. */
  constructor(id: string, page: number, onpage: (page: number) => void) {
    this.id = id;
    this.page = page;
    this.onpage = onpage;
  }

  /** Show the PDF in `area`; returns what takes it away. */
  abstract mount(area: HTMLDivElement): () => void;

  /** Step aside for a modal of the page (`covered`), or come back: a PDF drawn in the page is under the modal by its
   *  z-index; a native view is above all of the page, so it hides. */
  cover(covered: boolean): void {}

  /** In (1) or out (-1), or the width of the pane (0). */
  abstract zoom(step: number): void;

  /** Find the text of the find field: from the start, or `again` (the next or previous). */
  abstract search(again: boolean, previous?: boolean): void;

  protected abstract clearFind(): void;

  protected abstract goTo(target: unknown): void;

  /** The page in view: told by the reader. */
  protected turned(page: number) {
    this.current = page;
    this.onpage(page);
  }

  focusFind() {
    this.findInput?.select();
  }

  closeFind() {
    this.find = "";
    this.clearFind();
    this.findInput?.blur();
  }

  go(entry: Entry) {
    if (entry.target !== null && entry.target !== undefined) this.goTo(entry.target);
  }
}
