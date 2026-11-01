// The rows of a long list, only those in view drawn (and a few around, for a smooth scroll): the library's Table
// and FeedTable, as a table or as the Feeds tab's calm list, its rows taller. The list scrolls as one, down and across:
// a table's headers (TableHead, as high as a row of it) stay at its top, over the rows. The row in focus is scrolled
// into view, below the headers, when the keys move to it.

import { untrack } from "svelte";

/** the height of a row of a table, and of its headers, px */
export const ROW = 26;
/** rows drawn above and below the ones in view */
const EXTRA = 12;

/** What the rows are: how many; the index of the row in focus (-1: none); where the list is scrolled to when it
 *  comes, before the row in focus is scrolled into view; the height of a row and of the headers over them, px (none:
 *  a table's, `ROW` each) */
export interface RowsOf {
  count: () => number;
  focus: () => number;
  start?: () => number;
  row?: () => number;
  head?: () => number;
}

export class Rows {
  /** the list, scrolled: its headers, then its rows */
  table = $state<HTMLDivElement>();
  scrollTop = $state(0);
  /** how far across it is scrolled: what is at the right of the list in view is that far, and its width, in */
  scrollLeft = $state(0);
  /** the height of the list in view, its headers' with it */
  height = $state(0);
  /** the width of the list in view: the room of its columns */
  width = $state(0);

  readonly #count: () => number;
  readonly #row: () => number;

  constructor({ count, focus, start, row = () => ROW, head = () => ROW }: RowsOf) {
    this.#count = count;
    this.#row = row;
    if (start) {
      $effect(() => {
        const table = this.table;
        if (table) table.scrollTop = untrack(start);
      });
    }
    $effect(() => {
      const index = focus();
      const table = this.table;
      if (index < 0 || !table) return;
      const height = row();
      const top = index * height;
      // the rows in view: below the headers
      const rows = table.clientHeight - head();
      if (top < table.scrollTop) table.scrollTop = top;
      else if (top + height > table.scrollTop + rows) table.scrollTop = top + height - rows;
    });
  }

  /** the height of a row, px */
  get row() {
    return this.#row();
  }

  /** the first row drawn */
  get first() {
    return Math.max(0, Math.floor(this.scrollTop / this.row) - EXTRA);
  }

  /** the row after the last drawn */
  get last() {
    return Math.min(this.#count(), Math.ceil((this.scrollTop + this.height) / this.row) + EXTRA);
  }

  /** The list scrolled: where to, down. */
  scrolled = () => {
    if (this.table) [this.scrollTop, this.scrollLeft] = [this.table.scrollTop, this.table.scrollLeft];
    return this.scrollTop;
  };
}
