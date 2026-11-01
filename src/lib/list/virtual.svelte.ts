// The rows of a long table, only those in view drawn (and a few around, for a smooth scroll): the library's Table
// and FeedTable. The row in focus is scrolled into view when the keys move to it.

import { untrack } from "svelte";

/** the height of a row, px */
export const ROW = 26;
/** rows drawn above and below the ones in view */
const EXTRA = 12;

export class Rows {
  /** the scrolled body of the table */
  body = $state<HTMLDivElement>();
  scrollTop = $state(0);
  /** the height of the body */
  height = $state(0);

  readonly #count: () => number;

  /** `count`: the rows; `focus`: the index of the row in focus (-1: none); `start`: where the body is scrolled to
   *  when it comes, before the row in focus is scrolled into view */
  constructor(count: () => number, focus: () => number, start?: () => number) {
    this.#count = count;
    if (start) {
      $effect(() => {
        const body = this.body;
        if (body) body.scrollTop = untrack(start);
      });
    }
    $effect(() => {
      const index = focus();
      const body = this.body;
      if (index < 0 || !body) return;
      const top = index * ROW;
      if (top < body.scrollTop) body.scrollTop = top;
      else if (top + ROW > body.scrollTop + body.clientHeight) body.scrollTop = top + ROW - body.clientHeight;
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

  /** The body scrolled: where to. */
  scrolled = () => {
    if (this.body) this.scrollTop = this.body.scrollTop;
    return this.scrollTop;
  };
}
