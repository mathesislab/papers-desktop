// Reading the list and reviewing the shortlist: when an item shown counts as seen, and the steps of the Review.
// Pure, tested by scripts/feeds.test.ts (npm test).

import type { ViewRequest } from "./api.ts";

/** How long an item must stay selected to count as seen, ms. */
export const DWELL_MS = 800;

/**
 * Seen on dwell: a new item selected becomes seen once it has stayed selected `ms`. One passed over faster (j held
 * down, the arrows run through the list) stays new: its title went by, its details were not read. *Mark all above
 * as seen* (⇧Space) is for the titles skimmed.
 */
export class Dwell {
  private timer: ReturnType<typeof setTimeout> | null = null;
  private readonly onseen: (id: string) => void;
  private readonly ms: number;

  constructor(onseen: (id: string) => void, ms = DWELL_MS) {
    this.onseen = onseen;
    this.ms = ms;
  }

  /** The item selected now (none: no item), and whether it is new: the one before, if not seen yet, was passed. */
  show(id: string | null, isNew: boolean) {
    this.stop();
    if (id === null || !isNew) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      this.onseen(id);
    }, this.ms);
  }

  stop() {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }
}

/** What was done with an item of the review. */
export type Outcome = "added" | "dismissed" | "later";

/** The review of the shortlist: its items as it began, best score first, the one shown (`at`, the length when all
 *  are through), and what was done with each. */
export type Review = { ids: string[]; at: number; done: Record<string, Outcome> };

/** What the review asks the backend for: the shortlist, by score, every kind. */
export const REVIEW_REQUEST: ViewRequest = { query: "is:shortlisted", sort: "score", kinds: [] };

export const startReview = (ids: string[]): Review => ({ ids, at: 0, done: {} });

/** The item shown; none when all are through. */
export const reviewed = (r: Review): string | null => r.ids[r.at] ?? null;

/** j, k: the next or the one before; past the last, the end. */
export function stepReview(r: Review, by: number): Review {
  return { ...r, at: Math.max(0, Math.min(r.ids.length, r.at + by)) };
}

/** a, b, l, x: what was done with the item shown, then the next. */
export function decide(r: Review, outcome: Outcome): Review {
  const id = reviewed(r);
  if (id === null) return r;
  return { ...r, done: { ...r.done, [id]: outcome }, at: r.at + 1 };
}

/** "2 of 5", and how far as a fraction (1 at the end). */
export function progressOf(r: Review): { text: string; fraction: number } {
  const n = r.ids.length;
  if (!n) return { text: "nothing shortlisted", fraction: 1 };
  return { text: r.at < n ? `${r.at + 1} of ${n}` : `${n} of ${n}, done`, fraction: r.at / n };
}

/** What the end of the review says: "2 added, 1 dismissed, 2 for later". */
export function summaryOf(r: Review): string {
  const count = (o: Outcome) => Object.values(r.done).filter((d) => d === o).length;
  const parts = [`${count("added")} added`, `${count("dismissed")} dismissed`, `${count("later")} for later`];
  const left = r.ids.length - Object.keys(r.done).length;
  if (left) parts.push(`${left} passed`);
  return parts.join(", ");
}
