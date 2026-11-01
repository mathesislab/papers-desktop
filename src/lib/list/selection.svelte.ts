// The selection of a list (the Library, Feeds and Search tabs): the rows selected, the one in focus, which the
// details show and the keys move from, and the anchor that ⇧ extends from. The list is given to each call as its
// ids in order (`order.ts`), so the selection outlives a list that changes.

import { modClick } from "$lib/keys";
import { nextAfter, rangeOf, stepped } from "./order";

export class Selection {
  selected = $state(new Set<string>());
  /** the row the details show, and that the keys move from */
  focus = $state<string | null>(null);
  /** where ⇧ extends the selection from */
  anchor: string | null = null;
  /** with `hold`: the row of several selected that a press left so, for a drag or a double-click */
  #held: string | null = null;
  readonly #hold: boolean;

  /** `hold`: a press on one of several selected rows keeps them selected until the click ends (`release`), as in
   *  Finder, so that they can be dragged */
  constructor(hold = false) {
    this.#hold = hold;
  }

  /** One row alone selected, in focus; none: nothing. */
  only(id: string | null) {
    this.selected = new Set(id === null ? [] : [id]);
    this.anchor = this.focus = id;
  }

  /** A press on a row of the list `ids`: ⇧ selects the range from the anchor, ⌘ (Ctrl elsewhere) adds the row or
   *  takes it out, else the row alone. Whether it selected the row alone. */
  click(id: string, e: MouseEvent, ids: string[]): boolean {
    this.#held = null;
    let alone = false;
    if (e.shiftKey && this.anchor) {
      this.selected = new Set(rangeOf(ids, this.anchor, id));
    } else if (modClick(e)) {
      const s = new Set(this.selected);
      if (s.has(id)) s.delete(id);
      else s.add(id);
      this.selected = s;
      this.anchor = id;
    } else if (this.#hold && this.selected.has(id) && this.selected.size > 1) {
      this.#held = id;
    } else {
      this.selected = new Set([id]);
      this.anchor = id;
      alone = true;
    }
    this.focus = id;
    return alone;
  }

  /** The click that ends a press on one of several selected rows, which no drag followed: that row alone. */
  release(id: string) {
    if (this.#held === id) {
      this.selected = new Set([id]);
      this.anchor = id;
    }
    this.#held = null;
  }

  /** The focus moved by some rows, or to the first or last (`stepped`); the selection extended from the anchor, or
   *  the row alone. */
  move(ids: string[], to: number | "first" | "last", extend: boolean) {
    const next = stepped(ids, this.focus, to);
    if (next === null) return;
    if (extend && this.anchor) {
      this.selected = new Set(rangeOf(ids, this.anchor, next));
    } else {
      this.selected = new Set([next]);
      this.anchor = next;
    }
    this.focus = next;
  }

  /** The rows the keys and the menus act on, in the order of the list: the selection when the focus is in it, else
   *  the row in focus (in the list or not). */
  targets(ids: string[]): string[] {
    const focus = this.focus;
    if (focus !== null && this.selected.has(focus)) return ids.filter((id) => this.selected.has(id));
    return focus === null ? [] : [focus];
  }

  /** The list is now `ids`: the rows gone from it leave the selection, and the focus too unless `keepFocus` (an item
   *  shown that is not in the list, a row kept in place). */
  prune(ids: Iterable<string>, keepFocus = false) {
    const set = new Set(ids);
    if ([...this.selected].some((id) => !set.has(id))) this.selected = new Set([...this.selected].filter((id) => set.has(id)));
    if (!keepFocus && this.focus !== null && !set.has(this.focus)) this.focus = null;
  }

  /** Rows that go from the list `ids` (dismissed, deleted, moved away): they leave the selection; the focus, if it
   *  is one of them, goes to the next row that stays (`nextAfter`), alone selected. */
  nextAfter(ids: string[], removed: Set<string>) {
    if (this.focus !== null && removed.has(this.focus)) this.only(nextAfter(ids, removed));
    else if ([...this.selected].some((id) => removed.has(id))) this.selected = new Set([...this.selected].filter((id) => !removed.has(id)));
  }
}
