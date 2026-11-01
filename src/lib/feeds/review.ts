// Reviewing the shortlist: the steps of the Review. Pure, tested by scripts/feeds.test.ts (npm test).

import type { ViewRequest } from "./api.ts";

/** What was done with an item of the review. */
export type Outcome = "added" | "dismissed" | "later";

/** The review of the shortlist: its items as it began, best score first, the one shown (`at`, the length when all
 *  are through), and what was done with each. */
export type Review = { ids: string[]; at: number; done: Record<string, Outcome> };

/** What the review asks the backend for: the shortlist, by score. */
export const REVIEW_REQUEST: ViewRequest = { query: "is:shortlisted", sort: "score" };

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
  if (!n) return { text: "nothing starred", fraction: 1 };
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
