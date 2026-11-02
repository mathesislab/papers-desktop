// The rows of a long list, only those in view drawn (and a few around, for a smooth scroll): the library's Table
// and FeedTable, as a table or simple (its rows taller, no headers). The list scrolls as one, down and across: a
// table's headers (TableHead, as high as a row of it) stay at its top, over the rows. The row in focus is scrolled
// into view, below the headers, when the keys move to it (`scrolledTo`).

import { untrack } from "svelte";
import type { Look } from "./columns";

/** the height of a row of a table, and of its headers, px */
export const ROW = 26;
/** the height of a row of a simple list, px: its three lines */
const SIMPLE_ROW = 58;

/** The height of a row of a list of this look, px. */
export const rowOf = (look: Look) => (look === "simple" ? SIMPLE_ROW : ROW);
/** rows drawn above and below the ones in view */
const EXTRA = 12;

/** Where a list is scrolled to for its row at `top` (`height` high) to be in view, `rows` px of rows shown: as it
 *  is if it is; a step away (the keys moved), just enough; farther (a paper shown, the list back), the row a third
 *  of the way down. */
export function scrolledTo(top: number, height: number, scroll: number, rows: number): number {
  const above = top < scroll;
  const below = top + height > scroll + rows;
  if (!above && !below) return scroll;
  const near = top >= scroll - height && top + height <= scroll + rows + height;
  if (near) return above ? top : top + height - rows;
  return Math.max(0, top - Math.floor(rows / 3));
}

/** What the rows are: how many; the index of the row in focus (-1: none); where the list is scrolled to when it
 *  comes, before the row in focus is scrolled into view; the look of the list (none: a table), which says the height
 *  of a row and whether headers are over them */
export interface RowsOf {
  count: () => number;
  focus: () => number;
  start?: () => number;
  look?: () => Look;
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
  readonly #look: () => Look;

  constructor({ count, focus, start, look = () => "table" }: RowsOf) {
    this.#count = count;
    this.#look = look;
    if (start) {
      $effect(() => {
        const table = this.table;
        if (table) table.scrollTop = untrack(start);
      });
    }
    // once the list has its height (none while it is laid out: the row would go to its foot)
    $effect(() => {
      const index = focus();
      const table = this.table;
      const view = this.height;
      if (index < 0 || !table || view <= 0) return;
      untrack(() => {
        // the rows in view: below the headers, a table's
        const rows = view - (this.simple ? 0 : ROW);
        table.scrollTop = scrolledTo(index * this.row, this.row, table.scrollTop, rows);
      });
    });
  }

  /** the list simple: no headers over its rows */
  get simple() {
    return this.#look() === "simple";
  }

  /** the height of a row, px */
  get row() {
    return rowOf(this.#look());
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
