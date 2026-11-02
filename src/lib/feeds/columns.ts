// The columns of the lists of the Feeds and Search tabs (FeedTable): their names, their widths at first and least,
// those shown at first in each tab.

import type { Specs } from "../list/columns.ts";

/** a column of the list after its star and its mark */
export type FeedColumn = "score" | "cited" | "title" | "authors" | "journal" | "date" | "year" | "status";

/** the columns of the Feeds tab: their names, their widths at first (the title's is its least: it takes the room
 *  left), the least widths that fit a sorted header and its arrow and keep a title readable */
export const FEED_SPECS: Specs<FeedColumn> = {
  score: {
    name: "Score",
    hint: "Two bars, 0–100: on top how well it matches your interests, under how close it is to the papers of your library (Settings → Score)",
    width: 60,
    least: 60,
  },
  cited: { name: "Cited", width: 64, least: 56 },
  title: { name: "Title", width: 240, least: 100 },
  authors: { name: "Authors", width: 150 },
  journal: { name: "Journal / source", width: 150 },
  date: { name: "Date", width: 58, least: 58 },
  year: { name: "Year", width: 50, least: 48 },
  status: {
    name: "Status",
    hint: "Its version (◐ preprint, ◑ published, ● both; a post ✦ news or ✎ general), ✓ in the library, ✦ covered by news, ⏰ its reminder",
    width: 60,
    least: 60,
  },
};

/** the columns of the Feeds tab at first, in their order: those its menu offers. The journal and the title before
 *  the authors: a feed is looked through */
export const FEED_ORDER: FeedColumn[] = ["status", "score", "date", "journal", "title", "authors"];

/** the columns of the Search tab: as the library's, after the status and the score; the citations last. Wider titles
 *  and journals */
export const SEARCH_ORDER: FeedColumn[] = ["status", "score", "year", "authors", "title", "journal", "cited"];
export const SEARCH_SPECS: Specs<FeedColumn> = {
  ...FEED_SPECS,
  title: { ...FEED_SPECS.title, width: 280 },
  journal: { name: "Journal", width: 170 },
};

/** The columns as a layout of before saved them, its Badges column (words) the Status column (symbols) in its
 *  place, at its own width. */
function badgesAsStatus(saved: unknown): unknown {
  if (!Array.isArray(saved)) return saved;
  const keyOf = (c: unknown) => (c && typeof c === "object" ? (c as { key?: unknown }).key : null);
  return saved.map((c) => (keyOf(c) === "badges" ? { key: "status", width: FEED_SPECS.status.width } : c));
}

/** The columns as a layout of before saved them, its Similar column (`similar`, now the library part of the score)
 *  the Score column in its place and at its width, unless the score is shown too (then left out by
 *  `restoredColumns`, as any column no longer of the table). */
function similarAsScore(saved: unknown): unknown {
  if (!Array.isArray(saved)) return saved;
  const keyOf = (c: unknown) => (c && typeof c === "object" ? (c as { key?: unknown }).key : null);
  if (saved.some((c) => keyOf(c) === "score")) return saved;
  return saved.map((c) => (keyOf(c) === "similar" ? { ...c, key: "score" } : c));
}

/** The columns of the Feeds or Search tab as a layout of before saved them: those since renamed in their place
 *  (`badgesAsStatus`, `similarAsScore`). */
export const olderColumns = (saved: unknown): unknown => similarAsScore(badgesAsStatus(saved));
