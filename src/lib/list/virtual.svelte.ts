// The rows of a long table, only those in view drawn (and a few around, for a smooth scroll): the library's Table
// and FeedTable. The table scrolls as one, down and across: its headers (TableHead, as high as a row) stay at its
// top, over the rows. The row in focus is scrolled into view, below the headers, when the keys move to it.

import { untrack } from "svelte";

/** the height of a row, px */
export const ROW = 26;
/** rows drawn above and below the ones in view */
const EXTRA = 12;

export class Rows {
  /** the table, scrolled: its headers, then its rows */
  table = $state<HTMLDivElement>();
  scrollTop = $state(0);
  /** the height of the table in view, its headers' with it */
  height = $state(0);
  /** the width of the table in view: the room of its columns */
  width = $state(0);

  readonly #count: () => number;

  /** `count`: the rows; `focus`: the index of the row in focus (-1: none); `start`: where the table is scrolled to
   *  when it comes, before the row in focus is scrolled into view */
  constructor(count: () => number, focus: () => number, start?: () => number) {
    this.#count = count;
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
      const top = index * ROW;
      // the rows in view: below the headers, a row high
      const rows = table.clientHeight - ROW;
      if (top < table.scrollTop) table.scrollTop = top;
      else if (top + ROW > table.scrollTop + rows) table.scrollTop = top + ROW - rows;
    });
  }

  /** the first row drawn */
  get first() {
    return Math.max(0, Math.floor(this.scrollTop / ROW) - EXTRA);
  }

  /** the row after the last drawn */
  get last() {
    return Math.min(this.#count(), Math.ceil((this.scrollTop + this.height) / ROW) + EXTRA);
  }

  /** The table scrolled: where to. */
  scrolled = () => {
    if (this.table) this.scrollTop = this.table.scrollTop;
    return this.scrollTop;
  };
}
