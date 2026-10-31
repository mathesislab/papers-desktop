// The Feeds tab shows the items that the choices of its sidebar, the segment and the search select. As the library's
// (library/filters.ts), the choices are filters of their own, shown as chips, not text of the search field; the
// backend is asked for one query: the text typed followed by a term per choice (`is:new`, `kind:preprint`,
// `prof:"Ice sheets"`, `src:"The Cryosphere"`), all of which must hold. One choice per group of the sidebar. Pure
// functions, tested by scripts/feeds.test.ts (npm test).

import type { Feed, FeedCounts, ItemKind, Sort, SourceKind, SubscriptionInfo, ViewRequest } from "./api.ts";

/** the views of the sidebar: the items by their triage */
export type Shelf = "unread" | "shortlist" | "added" | "all";
/** Papers | Posts | All, above the list: the kinds asked for */
export type Segment = "papers" | "posts" | "all";

/** What the Feeds tab keeps between sessions (the layout's `feeds`). */
export type FeedFilters = {
  view: Shelf;
  /** the kind, interest and subscription (by title) chosen in the sidebar; none: any */
  kind: ItemKind | null;
  interest: string | null;
  source: string | null;
  segment: Segment;
  sort: Sort;
  /** the text of the search field, in the feeds' query language */
  query: string;
  /** the groups of the sources unfolded in the sidebar */
  open: string[];
};

export const NO_FEED_FILTERS: FeedFilters = {
  view: "unread",
  kind: null,
  interest: null,
  source: null,
  segment: "papers",
  sort: "score",
  query: "",
  open: ["journal"],
};

export const SHELVES: { view: Shelf; name: string; term: string | null }[] = [
  { view: "unread", name: "Unread", term: "is:new" },
  { view: "shortlist", name: "Shortlist", term: "is:shortlisted" },
  { view: "added", name: "Added", term: "is:added" },
  { view: "all", name: "All", term: null },
];

export const KINDS: { kind: ItemKind; name: string }[] = [
  { kind: "paper", name: "Papers" },
  { kind: "preprint", name: "Preprints" },
  { kind: "news", name: "News & press" },
  { kind: "blog", name: "Blogs" },
];

export const SEGMENTS: { segment: Segment; name: string; kinds: ItemKind[] }[] = [
  { segment: "papers", name: "Papers", kinds: ["paper", "preprint"] },
  { segment: "posts", name: "Posts", kinds: ["news", "blog"] },
  { segment: "all", name: "All", kinds: [] },
];

const kindsOf = (segment: Segment) => SEGMENTS.find((s) => s.segment === segment)?.kinds ?? [];

/** Whether the segment shows items of a kind. */
export const segmentHas = (segment: Segment, kind: ItemKind) => {
  const kinds = kindsOf(segment);
  return kinds.length === 0 || kinds.includes(kind);
};

/** A post: news or a blog; the rest are papers. */
export const isPost = (kind: ItemKind) => kind === "news" || kind === "blog";

const text = (v: unknown): v is string => typeof v === "string";

/** Filters as saved, any part missing or of a wrong type made the default. */
export function restoredFeeds(saved: Partial<FeedFilters> | null | undefined): FeedFilters {
  const d = NO_FEED_FILTERS;
  return {
    view: SHELVES.find((s) => s.view === saved?.view)?.view ?? d.view,
    kind: KINDS.find((k) => k.kind === saved?.kind)?.kind ?? null,
    interest: text(saved?.interest) ? saved.interest : null,
    source: text(saved?.source) ? saved.source : null,
    segment: SEGMENTS.find((s) => s.segment === saved?.segment)?.segment ?? d.segment,
    sort: saved?.sort === "newest" || saved?.sort === "score" ? saved.sort : d.sort,
    query: text(saved?.query) ? saved.query : d.query,
    open: Array.isArray(saved?.open) ? saved.open.filter(text) : d.open,
  };
}

/** A facet term: `kind:paper`, `prof:"Ice sheets"`. */
export function facet(name: string, value: string): string {
  return /^[\w.-]+$/.test(value) ? `${name}:${value}` : `${name}:"${value.replace(/"/g, "")}"`;
}

const DISMISSED = /(^|\s)is:dismissed(?=\s|$)/i;

/** The query the backend matches: the text typed, then a term per choice of the sidebar; dismissed items are left
 *  out unless the query asks for them. */
export function searchOf(f: FeedFilters): string {
  const terms = [
    f.query.trim(),
    SHELVES.find((s) => s.view === f.view)?.term ?? null,
    f.kind && facet("kind", f.kind),
    f.interest !== null && facet("prof", f.interest),
    f.source !== null && facet("src", f.source),
  ].filter((t): t is string => !!t);
  const query = terms.join(" ");
  return DISMISSED.test(query) ? query : [query, "-is:dismissed"].filter(Boolean).join(" ");
}

/** What `view` is asked for. */
export function requestOf(f: FeedFilters): ViewRequest {
  return { query: searchOf(f), sort: f.sort, kinds: kindsOf(f.segment) };
}

/** A kind clicked in the sidebar: that kind only, or any again; the segment follows when it does not show it. */
export function chooseKind(f: FeedFilters, kind: ItemKind): FeedFilters {
  if (f.kind === kind) return { ...f, kind: null };
  const segment = segmentHas(f.segment, kind) ? f.segment : isPost(kind) ? "posts" : "papers";
  return { ...f, kind, segment };
}

/** Another segment: a kind chosen that it does not show is let go. */
export function chooseSegment(f: FeedFilters, segment: Segment): FeedFilters {
  return { ...f, segment, kind: f.kind && !segmentHas(segment, f.kind) ? null : f.kind };
}

/** An interest or a subscription clicked: that one, or any again. */
export const chooseInterest = (f: FeedFilters, interest: string): FeedFilters => ({
  ...f,
  interest: f.interest === interest ? null : interest,
});
export const chooseSource = (f: FeedFilters, source: string): FeedFilters => ({
  ...f,
  source: f.source === source ? null : source,
});

/** A choice shown as a chip above the list; the × takes it off. */
export type FeedChip = { kind: "view" | "kind" | "interest" | "source" | "search"; text: string };

/** The chips of the choices made, in the order view › kind › interest › source › search. */
export function feedChips(f: FeedFilters): FeedChip[] {
  const list: FeedChip[] = [];
  if (f.view !== "all") list.push({ kind: "view", text: SHELVES.find((s) => s.view === f.view)?.name ?? f.view });
  if (f.kind) list.push({ kind: "kind", text: KINDS.find((k) => k.kind === f.kind)?.name ?? f.kind });
  if (f.interest !== null) list.push({ kind: "interest", text: f.interest });
  if (f.source !== null) list.push({ kind: "source", text: f.source });
  if (f.query.trim()) list.push({ kind: "search", text: `"${f.query.trim()}"` });
  return list;
}

/** A choice taken off: its chip's ×. */
export function withoutChip(f: FeedFilters, chip: FeedChip): FeedFilters {
  switch (chip.kind) {
    case "view":
      return { ...f, view: "all" };
    case "kind":
      return { ...f, kind: null };
    case "interest":
      return { ...f, interest: null };
    case "source":
      return { ...f, source: null };
    case "search":
      return { ...f, query: "" };
  }
}

/** The number of items of each view of the sidebar. */
export function shelfCounts(counts: FeedCounts | null): Record<Shelf, number> {
  const is = counts?.is ?? {};
  return { unread: is.new ?? 0, shortlist: is.shortlisted ?? 0, added: is.added ?? 0, all: counts?.all ?? 0 };
}

/** A group of the sources in the sidebar: the subscriptions of a kind, the sweeps of Crossref and OpenAlex apart. */
export type SourceGroup = { group: SourceKind | "sweeps"; name: string; count: number; sources: { title: string; count: number }[] };

const GROUPS: { group: SourceGroup["group"]; name: string }[] = [
  { group: "journal", name: "Journals" },
  { group: "preprints", name: "Preprint servers" },
  { group: "sweeps", name: "Sweeps (Crossref, OpenAlex)" },
  { group: "news", name: "News & press" },
  { group: "blog", name: "Blogs" },
];

const collator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });

/** The subscriptions in their groups, by title, each with the number of its items (`counts.sources`); the groups
 *  without any are left out. */
export function sourceGroups(subscriptions: SubscriptionInfo[], counts: Record<string, number>): SourceGroup[] {
  const groupOf = (s: SubscriptionInfo): SourceGroup["group"] => ("rss" in s.feed ? s.kind : "sweeps");
  return GROUPS.map(({ group, name }) => {
    const titles = [...new Set(subscriptions.filter((s) => groupOf(s) === group).map((s) => s.title))].sort(collator.compare);
    const sources = titles.map((title) => ({ title, count: counts[title] ?? 0 }));
    return { group, name, sources, count: sources.reduce((n, s) => n + s.count, 0) };
  }).filter((g) => g.sources.length > 0);
}

/** What a subscription follows, as the Subscribe sheet asks it: a feed's URL, a journal's ISSN for Crossref, a
 *  filter of OpenAlex. */
export type FeedType = "rss" | "crossref" | "openalex";

/** The feed of what was typed; none for an empty field. */
export function feedOf(type: FeedType, text: string): Feed | null {
  const value = text.trim();
  if (!value) return null;
  switch (type) {
    case "rss":
      return { rss: value };
    case "crossref":
      return { crossref: value };
    case "openalex":
      return { openalex: value };
  }
}

/** Whether two subscriptions follow the same: the URL without its scheme and a last slash, the ISSN without its
 *  hyphen, in any case. */
export function sameFeed(a: Feed, b: Feed): boolean {
  const key = (f: Feed) =>
    "rss" in f
      ? "rss " + f.rss.trim().toLowerCase().replace(/^https?:\/\//, "").replace(/\/$/, "")
      : "crossref" in f
        ? "crossref " + f.crossref.trim().toLowerCase().replace(/-/g, "")
        : "openalex " + f.openalex.trim().toLowerCase();
  return key(a) === key(b);
}

/** The colours of the interests, in the order of their names. */
const COLOURS = ["#2f9e6e", "#d9822b", "#8a5cd6", "#c2410c", "#0e7490", "#a16207"];

/** The interests by name, each with its colour: the sidebar's dot, the badge of a row. */
export function interestColours(names: string[]): Map<string, string> {
  return new Map([...names].sort(collator.compare).map((name, i) => [name, COLOURS[i % COLOURS.length]]));
}
