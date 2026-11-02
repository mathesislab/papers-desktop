// The state of the Search tab: the query asked of OpenAlex (the text of the search field, the Author(s) field as a:,
// the Year(s) field as y:, whose years Rust reads, and refuses when they are none), the same in plain words (the
// library's papers matched as a reference is, the services online), the recent searches, what the window's layout keeps of the tab,
// the works around a work. Pure functions, tested by scripts/search.test.ts (npm test).

import type { StarWords } from "../feeds/actions.ts";
import type { FeedDetail } from "../feeds/api.ts";
import { anyOf } from "../query.ts";
import type { Order } from "./api.ts";

/** The order of the works found: one OpenAlex gives, or the score (`score`): the works OpenAlex finds most relevant,
 *  those shown sorted by their score, the best first. */
export type SearchOrder = Order | "score";

/** The orders of the works found, as the tab offers them. */
export const ORDERS: { order: SearchOrder; text: string }[] = [
  { order: "newest", text: "Newest" },
  { order: "score", text: "Score" },
  { order: "cited", text: "Most cited" },
  { order: "relevance", text: "OpenAlex relevance" },
];

/** The works found in the order of the search, the library's papers among them: by score, the newest, the most
 *  cited first (those of no date or count last); OpenAlex's relevance as found (the library's papers first). A page
 *  more is sorted in. */
export function inOrder<T extends { score: number; date: string | null; cited: number | null }>(rows: T[], order: SearchOrder): T[] {
  const last = (x: number | null) => x ?? -Infinity;
  const time = (d: string | null) => (d ? Date.parse(d) : -Infinity);
  switch (order) {
    case "score":
      return [...rows].sort((a, b) => b.score - a.score);
    case "newest":
      return [...rows].sort((a, b) => time(b.date) - time(a.date));
    case "cited":
      return [...rows].sort((a, b) => last(b.cited) - last(a.cited));
    case "relevance":
      return rows;
  }
}

/** The order OpenAlex is asked for: the score sorts the most relevant. */
export const askedOf = (order: SearchOrder): Order => (order === "score" ? "relevance" : order);

/** The star of the tab, in its words: a work saved for later (starred in its folder). */
export const SAVED_FOR_LATER: StarWords = {
  star: "Save for later",
  unstar: "Remove from saved for later",
  starred: "Saved for later",
  hint: "Keep it, to find it again without searching: a new search does not drop it",
  row: { on: "Saved for later: unsave", off: "Save for later" },
};

/** The works of a page of a search (crates/feeds' `PER_SEARCH`). */
export const PER_PAGE = 25;

/** The recent searches kept, the latest first. */
export const RECENT_MAX = 20;

/** A search as typed: the text of the search field, the Author(s) and Year(s) fields, the order. */
export interface Typed {
  query: string;
  authors: string;
  year: string;
  order: SearchOrder;
}

/** The search the tab shows: the query asked (the authors and years in it), its order, the works found so far in their
 *  order, how many there are in all, the cursor of the next page. */
export interface Results {
  query: string;
  order: SearchOrder;
  ids: string[];
  total: number;
  next: string | null;
}

/** What the window's layout keeps of the Search tab: the fields, the recent searches, the search shown (its works
 *  are found again, without asking OpenAlex, when the tab opens). */
export interface SearchState extends Typed {
  recent: Typed[];
  results: Results | null;
}

export const NO_SEARCH: SearchState = { query: "", authors: "", year: "", order: "score", recent: [], results: null };

/** The names of the Author(s) field: separated by commas, semicolons, `&`, `and` or `AND`; each one required. */
export function namesOf(authors: string): string[] {
  return (authors.split(/[,;&]|\s+and\s+/i))
    .map((name) => name.trim().replace(/\s+/g, " "))
    .filter(Boolean);
}

/** The query asked of OpenAlex: the text of the search field, each name as `a:` (quoted when of several words), the
 *  Year(s) field as `y:` (`2015, 2020`, `2000 - 2010`: spaces left out). "" when all are blank. */
export function composed(s: Pick<Typed, "query" | "authors" | "year">): string {
  const years = s.year.replace(/\s+/g, "");
  const names = namesOf(s.authors).map((name) => (name.includes(" ") ? `a:"${name}"` : `a:${name}`));
  return [s.query.trim(), ...names, years && `y:${years}`].filter(Boolean).join(" ");
}

/** The search in plain words, as a reference is written: the text, the names, the years (the library's papers are
 *  matched to it as a reference is). */
export function plainWords(s: Pick<Typed, "query" | "authors" | "year">): string {
  return [s.query.trim(), ...namesOf(s.authors), s.year.trim()].filter(Boolean).join(" ");
}

/** The same search: the same text (spaces aside), names and years; the order is not part of it. */
const same = (a: Typed, b: Typed) =>
  a.query.trim().replace(/\s+/g, " ") === b.query.trim().replace(/\s+/g, " ") &&
  namesOf(a.authors).join(",") === namesOf(b.authors).join(",") &&
  a.year.replace(/\s+/g, "") === b.year.replace(/\s+/g, "");

/** The recent searches after one more: it first, taking the place of the same search before (its order now), the
 *  list at most `max` long. A blank search is not kept. */
export function withRecent(list: Typed[], search: Typed, max = RECENT_MAX): Typed[] {
  const entry = {
    query: search.query.trim(),
    authors: namesOf(search.authors).join(", "),
    year: search.year.replace(/\s+/g, ""),
    order: search.order,
  };
  if (!entry.query && !entry.authors && !entry.year) return list;
  return [entry, ...list.filter((r) => !same(r, entry))].slice(0, max);
}

/** A recent search as the sidebar shows it: `greenland ice sheet · Smith, Fricker · 2000-2010 · Most cited`; the
 *  order at first not said. */
export function recentText(r: Typed): string {
  const order = r.order === NO_SEARCH.order ? null : ORDERS.find((o) => o.order === r.order)?.text;
  return [r.query.trim(), r.authors, r.year, order].filter(Boolean).join(" · ");
}

const text = (v: unknown): v is string => typeof v === "string";
const isOrder = (v: unknown): v is SearchOrder => ORDERS.some((o) => o.order === v);

/** A search as saved, or null when it is not one. */
function typed(v: unknown): Typed | null {
  if (!v || typeof v !== "object") return null;
  const r = v as Partial<Typed>;
  if (!text(r.query) || !text(r.year)) return null;
  return { query: r.query, authors: text(r.authors) ? r.authors : "", year: r.year, order: isOrder(r.order) ? r.order : NO_SEARCH.order };
}

/** The search shown as saved, or null when it is not one. */
function results(v: unknown): Results | null {
  if (!v || typeof v !== "object") return null;
  const r = v as Partial<Results>;
  if (!text(r.query) || !Array.isArray(r.ids) || typeof r.total !== "number") return null;
  return {
    query: r.query,
    order: isOrder(r.order) ? r.order : "relevance",
    ids: r.ids.filter(text),
    total: r.total,
    next: text(r.next) ? r.next : null,
  };
}

/** The Search tab as the layout saved it, any part missing or of a wrong type made the default. */
export function restoredSearch(saved: Partial<SearchState> | null | undefined): SearchState {
  const d = NO_SEARCH;
  const recent = Array.isArray(saved?.recent) ? saved.recent.map(typed).filter((r): r is Typed => r !== null) : d.recent;
  return {
    query: text(saved?.query) ? saved.query : d.query,
    authors: text(saved?.authors) ? saved.authors : d.authors,
    year: text(saved?.year) ? saved.year : d.year,
    order: isOrder(saved?.order) ? saved.order : d.order,
    recent: recent.slice(0, RECENT_MAX),
    results: results(saved?.results),
  };
}

/** The query of the works around a work: those that cite it (`cites`, *Cited by*), or those it cites (`citedby`,
 *  *References*), by its OpenAlex id, else its DOI. None without either. */
export function aroundQuery(d: Pick<FeedDetail, "openalex" | "doi">, facet: "cites" | "citedby"): string | null {
  const id = d.openalex ?? d.doi;
  return id ? anyOf(facet, [id]) : null;
}

const NUMBER = new Intl.NumberFormat("en-US");

/** How many works were found: "1,234 works". */
export const totalText = (total: number) => `${NUMBER.format(total)} ${total === 1 ? "work" : "works"}`;

/** The button at the end of the list: "Show 25 more", fewer for the last page. */
export const moreText = (shown: number, total: number) => `Show ${Math.max(1, Math.min(PER_PAGE, total - shown))} more`;

/** The rows of a page after those shown, without those already there. */
export function appended<T extends { id: string }>(rows: T[], page: T[]): T[] {
  const ids = new Set(rows.map((r) => r.id));
  return [...rows, ...page.filter((r) => !ids.has(r.id))];
}
