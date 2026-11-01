// The Feeds tab shows the items that the choices of its sidebar and the search select. As the library's
// (library/filters.ts), the choices are filters of their own, shown as chips, not text of the search field; the
// backend is asked for one query: a term per group of choices (`is:new`, `kind:paper,preprint`, `prof:"Ice sheets"`,
// `src:"The Cryosphere",GRL`) followed by the text typed, all of which must hold, an item being any of the choices
// of a group. A click chooses one of a group, ⌘-click several. Pure functions, tested by
// scripts/feeds.test.ts (npm test).

import { FIELDS, anyOf, queryTerms } from "../query.ts";
import type { FeedCounts, ItemKind, Sort, ViewRequest } from "./api.ts";
import { ITEM_KINDS, isItemKind } from "./kinds.ts";

/** the views of the sidebar: the items by their triage */
export type Shelf = "unread" | "starred" | "snoozed" | "added" | "all";

/** What the Feeds tab keeps between sessions (the layout's `feeds`). */
export type FeedFilters = {
  view: Shelf;
  /** the kinds, interests and subscriptions (by title) chosen in the sidebar, any of which an item may be; none: any */
  kinds: ItemKind[];
  interests: string[];
  sources: string[];
  sort: Sort;
  /** the text of the search field, in the feeds' query language */
  query: string;
  /** the groups of the sources unfolded in the sidebar */
  open: string[];
};

export const NO_FEED_FILTERS: FeedFilters = {
  view: "unread",
  kinds: [],
  interests: [],
  sources: [],
  sort: "score",
  query: "",
  open: ["journal"],
};

export const SHELVES: { view: Shelf; name: string; term: string | null }[] = [
  { view: "unread", name: "Unread", term: "is:new" },
  { view: "starred", name: "Starred", term: "is:starred" },
  { view: "snoozed", name: "Snoozed", term: "is:snoozed" },
  { view: "added", name: "Added", term: "is:added" },
  { view: "all", name: "All", term: null },
];

const text = (v: unknown): v is string => typeof v === "string";

/** The orders of the list, in the order the count above it goes through them. */
export const SORTS: { sort: Sort; text: string }[] = [
  { sort: "score", text: "best score first" },
  { sort: "like", text: "most similar to the library first" },
  { sort: "newest", text: "newest first" },
];

/** The order after this one, for the count above the list. */
export const nextSort = (sort: Sort): Sort => SORTS[(SORTS.findIndex((s) => s.sort === sort) + 1) % SORTS.length].sort;

/** The values of a choice as saved: several, or one of an older version (`kind`, `interest`, `source`). */
function values<T extends string>(many: unknown, one: unknown, valid: (v: unknown) => v is T): T[] {
  const list = Array.isArray(many) ? many : one !== undefined && one !== null ? [one] : [];
  return [...new Set(list.filter(valid))];
}

/** the views of the sidebar by their names of before */
const SHELVES_BEFORE: Record<string, Shelf> = { shortlist: "starred" };

/** Filters as saved, any part missing or of a wrong type made the default; a single choice of an older version
 *  becomes the only one of its group, a view of an older name the view of now. */
export function restoredFeeds(
  saved: (Partial<FeedFilters> & { kind?: unknown; interest?: unknown; source?: unknown }) | null | undefined,
): FeedFilters {
  const d = NO_FEED_FILTERS;
  const view = typeof saved?.view === "string" ? (SHELVES_BEFORE[saved.view] ?? saved.view) : null;
  return {
    view: SHELVES.find((s) => s.view === view)?.view ?? d.view,
    kinds: values(saved?.kinds, saved?.kind, isItemKind),
    interests: values(saved?.interests, saved?.interest, text),
    sources: values(saved?.sources, saved?.source, text),
    sort: SORTS.find((s) => s.sort === saved?.sort)?.sort ?? d.sort,
    query: text(saved?.query) ? saved.query : d.query,
    open: Array.isArray(saved?.open) ? saved.open.filter(text) : d.open,
  };
}

/** The field names of the feeds' queries: the core's, and the facets and numbers of crates/feeds/src/query.rs. */
const FEED_FIELDS = [...FIELDS, "kind", "is", "src", "prof", "score", "like"];

/** The query of the choices: a term per group of the sidebar, then the text typed (`is:new kind:paper,preprint
 *  prof:AMOC melt`). The whole query of the tab: what a saved view keeps (see `decompose`, which takes the choices
 *  back: they come first). */
export function queryOf(f: FeedFilters): string {
  return [
    SHELVES.find((s) => s.view === f.view)?.term ?? "",
    anyOf("kind", f.kinds),
    anyOf("prof", f.interests),
    anyOf("src", f.sources),
    f.query.trim(),
  ]
    .filter(Boolean)
    .join(" ");
}

/** What `view` is asked for: the query of the choices (the backend leaves the dismissed items out unless it names
 *  them), the order. */
export function requestOf(f: FeedFilters): ViewRequest {
  return { query: queryOf(f), sort: f.sort };
}

/** A value of a group clicked: that one alone, or none when it was so already; with ⌘ (`toggle`), added to the
 *  choices or taken out of them. */
function chosen<T>(values: T[], value: T, toggle: boolean): T[] {
  if (toggle) return values.includes(value) ? values.filter((v) => v !== value) : [...values, value];
  return values.length === 1 && values[0] === value ? [] : [value];
}

/** A kind, an interest or a subscription clicked (see `chosen`). */
export const chooseKind = (f: FeedFilters, kind: ItemKind, toggle = false): FeedFilters => ({
  ...f,
  kinds: chosen(f.kinds, kind, toggle),
});
export const chooseInterest = (f: FeedFilters, interest: string, toggle = false): FeedFilters => ({
  ...f,
  interests: chosen(f.interests, interest, toggle),
});
export const chooseSource = (f: FeedFilters, source: string, toggle = false): FeedFilters => ({
  ...f,
  sources: chosen(f.sources, source, toggle),
});

/** What a query of the tab chooses. */
type Choices = Pick<FeedFilters, "view" | "kinds" | "interests" | "sources" | "query">;

/** The choices of a query, as the sidebar and the field show them: its first `is:` term of a view, `kind:`, `prof:`
 *  and `src:` terms (not excluding) become the choices of their groups, the rest the text typed, in its order.
 *  `queryOf` gives an equivalent query back. */
export function decompose(query: string): Choices {
  const f: Choices = { view: "all", kinds: [], interests: [], sources: [], query: "" };
  const taken = new Set<string>();
  const rest: string[] = [];
  for (const term of queryTerms(query, FEED_FIELDS)) {
    const vals = [...new Set(term.values.map((v) => v.trim()).filter(Boolean))];
    const field = term.field ?? "";
    const free = !term.negated && vals.length > 0 && !taken.has(field);
    const lower = vals.map((v) => v.toLowerCase());
    const shelf = SHELVES.find((s) => s.term === `is:${lower[0] === "unread" ? "new" : lower[0]}`);
    if (free && field === "is" && vals.length === 1 && shelf) {
      f.view = shelf.view;
    } else if (free && field === "kind" && lower.every(isItemKind)) {
      f.kinds = [...new Set(lower)] as ItemKind[];
    } else if (free && field === "prof") {
      f.interests = vals;
    } else if (free && field === "src") {
      f.sources = vals;
    } else {
      rest.push(term.text);
      continue;
    }
    taken.add(field);
  }
  f.query = rest.join(" ");
  return f;
}

/** Whether two queries of the feeds are the same: the same choices and text, in any order of the groups. */
export function sameQuery(a: string, b: string): boolean {
  const canonical = (q: string) => queryOf({ ...NO_FEED_FILTERS, ...decompose(q) });
  return canonical(a) === canonical(b);
}

/** A saved view applied: its choices, text and order in place of the tab's (the groups unfolded stay). */
export const applied = (f: FeedFilters, search: { query: string; sort: Sort }): FeedFilters => ({
  ...f,
  ...decompose(search.query),
  sort: search.sort,
});

/** Whether a saved view is what the tab shows: the same query, the same order. */
export const isShown = (f: FeedFilters, search: { query: string; sort: Sort }) =>
  search.sort === f.sort && sameQuery(search.query, queryOf(f));

/** A choice shown as a chip above the list; the × takes it off. */
export type FeedChip = { kind: "view" | "kind" | "interest" | "source" | "search"; text: string };

/** A chip of several values: `Source: TC, GRL`; one value alone. */
const several = (name: string, values: string[]) => (values.length > 1 ? `${name}: ${values.join(", ")}` : (values[0] ?? ""));

/** The chips of the choices made, in the order view › kinds › interests › sources › search. */
export function feedChips(f: FeedFilters): FeedChip[] {
  const list: FeedChip[] = [];
  if (f.view !== "all") list.push({ kind: "view", text: SHELVES.find((s) => s.view === f.view)?.name ?? f.view });
  if (f.kinds.length) list.push({ kind: "kind", text: several("Kind", f.kinds.map((k) => ITEM_KINDS[k].plural)) });
  if (f.interests.length) list.push({ kind: "interest", text: several("Interest", f.interests) });
  if (f.sources.length) list.push({ kind: "source", text: several("Source", f.sources) });
  if (f.query.trim()) list.push({ kind: "search", text: `"${f.query.trim()}"` });
  return list;
}

/** A choice taken off: its chip's ×. */
export function withoutChip(f: FeedFilters, chip: FeedChip): FeedFilters {
  switch (chip.kind) {
    case "view":
      return { ...f, view: "all" };
    case "kind":
      return { ...f, kinds: [] };
    case "interest":
      return { ...f, interests: [] };
    case "source":
      return { ...f, sources: [] };
    case "search":
      return { ...f, query: "" };
  }
}

/** The number of items of each view of the sidebar. */
export function shelfCounts(counts: FeedCounts | null): Record<Shelf, number> {
  const is = counts?.is ?? {};
  return {
    unread: is.new ?? 0,
    starred: is.starred ?? 0,
    snoozed: is.snoozed ?? 0,
    added: is.added ?? 0,
    all: counts?.all ?? 0,
  };
}

const collator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });

/** The colours of the interests, in the order of their names. */
const COLOURS = ["#2f9e6e", "#d9822b", "#8a5cd6", "#c2410c", "#0e7490", "#a16207"];

/** The interests by name, each with its colour: the sidebar's dot, the badge of a row. */
export function interestColours(names: string[]): Map<string, string> {
  return new Map([...names].sort(collator.compare).map((name, i) => [name, COLOURS[i % COLOURS.length]]));
}
