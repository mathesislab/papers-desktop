// The Search tab: the query asked of OpenAlex (the text of the search field and the Year field), the recent
// searches, what the window's layout keeps of the tab, the works around a work. Pure functions, tested by
// scripts/search.test.ts (npm test).

import type { FeedDetail } from "../feeds/api.ts";
import type { Order } from "./api.ts";

/** The orders of the works found, as the tab offers them. */
export const ORDERS: { order: Order; text: string }[] = [
  { order: "relevance", text: "Relevance" },
  { order: "cited", text: "Most cited" },
  { order: "newest", text: "Newest" },
];

/** The works of a page of a search (crates/feeds' `PER_SEARCH`). */
export const PER_PAGE = 25;

/** The recent searches kept, the latest first. */
export const RECENT_MAX = 20;

/** A search as typed: the text of the search field, the Year field, the order. */
export interface Asked {
  query: string;
  year: string;
  order: Order;
}

/** The search the tab shows: the query asked (the Year field in it), its order, the works found so far in their
 *  order, how many there are in all, the cursor of the next page. */
export interface Results {
  query: string;
  order: Order;
  ids: string[];
  total: number;
  next: string | null;
}

/** What the window's layout keeps of the Search tab: the fields, the recent searches, the search shown (its works
 *  are found again, without asking OpenAlex, when the tab opens). */
export interface SearchState extends Asked {
  recent: Asked[];
  results: Results | null;
}

export const NO_SEARCH: SearchState = { query: "", year: "", order: "relevance", recent: [], results: null };

const YEAR = /^(\d{4})?-(\d{4})?$|^(\d{4})$/;

/** The years of the Year field as `y:` takes them, spaces left out: `2005`, `2000-2010`, `2000-`, `-2010`; "" for
 *  none; null for what is not years (or a span backwards). */
export function yearValue(year: string): string | null {
  const value = year.replace(/\s+/g, "");
  if (!value) return "";
  const m = YEAR.exec(value);
  if (!m || value === "-") return null;
  if (m[1] && m[2] && Number(m[1]) > Number(m[2])) return null;
  return value;
}

/** What is wrong with the Year field, said; none when it is right or blank. */
export const yearError = (year: string) =>
  yearValue(year) === null ? `Year: “${year.trim()}” is not 2005, 2000-2010, 2000- or -2010` : null;

/** The query asked of OpenAlex: the text of the search field, then the Year field as `y:`. "" when both are blank;
 *  null when the year is wrong. */
export function composed(query: string, year: string): string | null {
  const years = yearValue(year);
  if (years === null) return null;
  return [query.trim(), years && `y:${years}`].filter(Boolean).join(" ");
}

/** The same search: the same text (spaces aside) and years; the order is not part of it. */
const same = (a: Asked, b: Asked) =>
  a.query.trim().replace(/\s+/g, " ") === b.query.trim().replace(/\s+/g, " ") && a.year.replace(/\s+/g, "") === b.year.replace(/\s+/g, "");

/** The recent searches after one more: it first, taking the place of the same search before (its order now), the
 *  list at most `max` long. A blank search is not kept. */
export function withRecent(list: Asked[], search: Asked, max = RECENT_MAX): Asked[] {
  const entry = { query: search.query.trim(), year: search.year.replace(/\s+/g, ""), order: search.order };
  if (!entry.query && !entry.year) return list;
  return [entry, ...list.filter((r) => !same(r, entry))].slice(0, max);
}

/** A recent search as the sidebar shows it: `greenland ice sheet · 2000-2010 · Most cited`. */
export function recentText(r: Asked): string {
  const order = r.order === "relevance" ? null : ORDERS.find((o) => o.order === r.order)?.text;
  return [r.query.trim(), r.year, order].filter(Boolean).join(" · ");
}

const text = (v: unknown): v is string => typeof v === "string";
const isOrder = (v: unknown): v is Order => ORDERS.some((o) => o.order === v);

/** A search as saved, or null when it is not one. */
function asked(v: unknown): Asked | null {
  if (!v || typeof v !== "object") return null;
  const r = v as Partial<Asked>;
  if (!text(r.query) || !text(r.year)) return null;
  return { query: r.query, year: r.year, order: isOrder(r.order) ? r.order : "relevance" };
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
  const recent = Array.isArray(saved?.recent) ? saved.recent.map(asked).filter((r): r is Asked => r !== null) : d.recent;
  return {
    query: text(saved?.query) ? saved.query : d.query,
    year: text(saved?.year) ? saved.year : d.year,
    order: isOrder(saved?.order) ? saved.order : d.order,
    recent: recent.slice(0, RECENT_MAX),
    results: results(saved?.results),
  };
}

/** A value of a filter as the query reads it: quoted when it has a space, a comma or a quote. */
const valueOf = (v: string) => (/[\s,"]/.test(v) ? `"${v.replace(/"/g, "")}"` : v);

/** The query of the works around a work: those that cite it (`cites`, *Cited by*), or those it cites (`citedby`,
 *  *References*), by its OpenAlex id, else its DOI. None without either. */
export function aroundQuery(d: Pick<FeedDetail, "openalex" | "doi">, facet: "cites" | "citedby"): string | null {
  const id = d.openalex ?? d.doi;
  return id ? `${facet}:${valueOf(id)}` : null;
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
