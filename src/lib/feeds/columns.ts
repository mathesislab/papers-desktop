// The columns of the lists of the Feeds and Search tabs (FeedTable): their names, their widths at first and least,
// those shown at first in each tab; the looks of the Feeds tab's list, a table or calm, and the height of a calm row.

import type { Specs } from "../list/columns.ts";

/** a column of the list after its star and its mark */
export type FeedColumn = "score" | "cited" | "similar" | "title" | "authors" | "journal" | "date" | "year" | "badges";

/** the columns of the Feeds tab: their names, their widths at first (the title's is its least: it takes the room
 *  left), the least widths that fit a sorted header and its arrow and keep a title readable */
export const FEED_SPECS: Specs<FeedColumn> = {
  score: { name: "Score", hint: "How well it matches your interests (Settings → Feeds)", width: 60, least: 60 },
  cited: { name: "Cited", width: 64, least: 56 },
  similar: { name: "Similar", hint: "How close it is to the papers of your library, 0–100", width: 70, least: 70 },
  title: { name: "Title", width: 240, least: 100 },
  authors: { name: "Authors", width: 150 },
  journal: { name: "Journal / source", width: 150 },
  date: { name: "Date", width: 58, least: 58 },
  year: { name: "Year", width: 50, least: 48 },
  badges: { name: "Badges", width: 190 },
};

/** the columns of the Feeds tab at first, in their order: those its menu offers */
export const FEED_ORDER: FeedColumn[] = ["date", "journal", "title", "authors", "similar", "score", "badges"];

/** the columns of the Search tab: the citations first, the year, no score; wider titles and journals */
export const SEARCH_ORDER: FeedColumn[] = ["cited", "similar", "title", "authors", "journal", "year", "badges"];
export const SEARCH_SPECS: Specs<FeedColumn> = {
  ...FEED_SPECS,
  title: { ...FEED_SPECS.title, width: 280 },
  journal: { name: "Journal", width: 170 },
  badges: { ...FEED_SPECS.badges, width: 110 },
};

/** How the Feeds tab shows its list: calm, a row of three lines (the source, the title, the authors) after the date,
 *  no headers; or a table of the columns, whose headers sort, move and resize them. The Search tab's is a table. */
export type Look = "calm" | "table";

/** the height of a row of the calm list, px: its three lines */
export const CALM_ROW = 58;
