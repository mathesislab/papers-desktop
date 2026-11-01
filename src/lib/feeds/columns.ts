// The columns of the lists of the Feeds and Search tabs (FeedTable): their names, their widths at first and least,
// those shown at first in each tab.

import type { Specs } from "../list/columns.ts";

/** a column of the list after its star and its mark */
export type FeedColumn = "score" | "cited" | "title" | "authors" | "journal" | "date" | "year" | "badges";

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
  badges: { name: "Badges", width: 190 },
};

/** the columns of the Feeds tab at first, in their order: those its menu offers */
export const FEED_ORDER: FeedColumn[] = ["date", "journal", "title", "authors", "score", "badges"];

/** the columns of the Search tab: the citations first, the score, the year; wider titles and journals */
export const SEARCH_ORDER: FeedColumn[] = ["cited", "score", "title", "authors", "journal", "year", "badges"];
export const SEARCH_SPECS: Specs<FeedColumn> = {
  ...FEED_SPECS,
  title: { ...FEED_SPECS.title, width: 280 },
  journal: { name: "Journal", width: 170 },
  badges: { ...FEED_SPECS.badges, width: 110 },
};
