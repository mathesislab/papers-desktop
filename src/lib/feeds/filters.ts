// The Feeds tab shows the items that the choices of its sidebar, the segment and the search select. As the library's
// (library/filters.ts), the choices are filters of their own, shown as chips, not text of the search field; the
// backend is asked for one query: a term per group of choices (`is:new`, `kind:paper,preprint`, `prof:"Ice sheets"`,
// `src:"The Cryosphere",GRL`) followed by the text typed, all of which must hold, an item being any of the choices
// of a group. A click chooses one of a group, ⌘-click several. Pure functions, tested by
// scripts/feeds.test.ts (npm test).

import { FIELDS, anyOf, queryTerms } from "../query.ts";
import type {
  Feed,
  FeedCounts,
  ItemKind,
  Journal,
  JournalSite,
  Sort,
  SourceKind,
  SubscriptionInfo,
  ViewRequest,
} from "./api.ts";

/** the views of the sidebar: the items by their triage */
export type Shelf = "unread" | "shortlist" | "added" | "all";
/** Papers | Posts | All, above the list: the kinds asked for */
export type Segment = "papers" | "posts" | "all";

/** What the Feeds tab keeps between sessions (the layout's `feeds`). */
export type FeedFilters = {
  view: Shelf;
  /** the kinds, interests and subscriptions (by title) chosen in the sidebar, any of which an item may be; none: any */
  kinds: ItemKind[];
  interests: string[];
  sources: string[];
  segment: Segment;
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

/** The segment that shows the kinds: Papers, Posts, else All. */
const segmentOf = (kinds: ItemKind[]): Segment =>
  kinds.every((k) => !isPost(k)) ? "papers" : kinds.every(isPost) ? "posts" : "all";

/** Whether the segment shows items of a kind. */
export const segmentHas = (segment: Segment, kind: ItemKind) => {
  const kinds = kindsOf(segment);
  return kinds.length === 0 || kinds.includes(kind);
};

/** A post: news or a blog; the rest are papers. */
export const isPost = (kind: ItemKind) => kind === "news" || kind === "blog";

const text = (v: unknown): v is string => typeof v === "string";

/** The orders of the list, in the order the count above it goes through them. */
export const SORTS: { sort: Sort; text: string }[] = [
  { sort: "score", text: "best score first" },
  { sort: "like", text: "most like the library first" },
  { sort: "newest", text: "newest first" },
];

/** The order after this one, for the count above the list. */
export const nextSort = (sort: Sort): Sort => SORTS[(SORTS.findIndex((s) => s.sort === sort) + 1) % SORTS.length].sort;

const isKind = (v: unknown): v is ItemKind => KINDS.some((k) => k.kind === v);

/** The values of a choice as saved: several, or one of an older version (`kind`, `interest`, `source`). */
function values<T extends string>(many: unknown, one: unknown, valid: (v: unknown) => v is T): T[] {
  const list = Array.isArray(many) ? many : one !== undefined && one !== null ? [one] : [];
  return [...new Set(list.filter(valid))];
}

/** Filters as saved, any part missing or of a wrong type made the default; a single choice of an older version
 *  becomes the only one of its group. */
export function restoredFeeds(
  saved: (Partial<FeedFilters> & { kind?: unknown; interest?: unknown; source?: unknown }) | null | undefined,
): FeedFilters {
  const d = NO_FEED_FILTERS;
  return {
    view: SHELVES.find((s) => s.view === saved?.view)?.view ?? d.view,
    kinds: values(saved?.kinds, saved?.kind, isKind),
    interests: values(saved?.interests, saved?.interest, text),
    sources: values(saved?.sources, saved?.source, text),
    segment: SEGMENTS.find((s) => s.segment === saved?.segment)?.segment ?? d.segment,
    sort: SORTS.find((s) => s.sort === saved?.sort)?.sort ?? d.sort,
    query: text(saved?.query) ? saved.query : d.query,
    open: Array.isArray(saved?.open) ? saved.open.filter(text) : d.open,
  };
}

/** The field names of the feeds' queries: the core's, and the facets and numbers of crates/feeds/src/query.rs. */
const FEED_FIELDS = [...FIELDS, "kind", "is", "src", "prof", "score", "like"];

/** Whether a query has an `is:` term on dismissed items (`is:dismissed`, `-is:seen,dismissed`). */
const asksDismissed = (query: string) =>
  queryTerms(query, FEED_FIELDS).some((t) => t.field === "is" && t.values.some((v) => v.trim().toLowerCase() === "dismissed"));

/** The query of the choices: a term per group of the sidebar, the segment's kinds when no kind is chosen, then the
 *  text typed (`is:new kind:paper,preprint prof:AMOC melt`). The whole query of the tab, its segment too: what a saved
 *  search keeps (see `decompose`, which takes the choices back: they come first). */
export function queryOf(f: FeedFilters): string {
  return [
    SHELVES.find((s) => s.view === f.view)?.term ?? "",
    anyOf("kind", f.kinds.length ? f.kinds : kindsOf(f.segment)),
    anyOf("prof", f.interests),
    anyOf("src", f.sources),
    f.query.trim(),
  ]
    .filter(Boolean)
    .join(" ");
}

/** A query of the feeds as shown: dismissed items left out unless it asks for them. */
export const live = (query: string) =>
  asksDismissed(query) ? query : [query, "-is:dismissed"].filter(Boolean).join(" ");

/** The query the backend matches: the query of the choices, live. */
export const searchOf = (f: FeedFilters): string => live(queryOf(f));

/** What `view` is asked for. */
export function requestOf(f: FeedFilters): ViewRequest {
  return { query: searchOf(f), sort: f.sort, kinds: kindsOf(f.segment) };
}

/** A value of a group clicked: that one alone, or none when it was so already; with ⌘ (`toggle`), added to the
 *  choices or taken out of them. */
function chosen<T>(values: T[], value: T, toggle: boolean): T[] {
  if (toggle) return values.includes(value) ? values.filter((v) => v !== value) : [...values, value];
  return values.length === 1 && values[0] === value ? [] : [value];
}

/** A kind clicked in the sidebar (see `chosen`); the segment follows when it does not show them. */
export function chooseKind(f: FeedFilters, kind: ItemKind, toggle = false): FeedFilters {
  const kinds = chosen(f.kinds, kind, toggle);
  return { ...f, kinds, segment: kinds.every((k) => segmentHas(f.segment, k)) ? f.segment : segmentOf(kinds) };
}

/** Another segment: the kinds chosen that it does not show are let go. */
export function chooseSegment(f: FeedFilters, segment: Segment): FeedFilters {
  return { ...f, segment, kinds: f.kinds.filter((k) => segmentHas(segment, k)) };
}

/** An interest or a subscription clicked (see `chosen`). */
export const chooseInterest = (f: FeedFilters, interest: string, toggle = false): FeedFilters => ({
  ...f,
  interests: chosen(f.interests, interest, toggle),
});
export const chooseSource = (f: FeedFilters, source: string, toggle = false): FeedFilters => ({
  ...f,
  sources: chosen(f.sources, source, toggle),
});

/** What a query of the tab chooses. */
type Choices = Pick<FeedFilters, "view" | "kinds" | "interests" | "sources" | "segment" | "query">;

/** The choices of a query, as the sidebar, the segment and the field show them: its first `is:` term of a view,
 *  `kind:`, `prof:` and `src:` terms (not excluding) become the choices of their groups, a `kind:` of a segment's
 *  kinds that segment, the rest the text typed, in its order. `queryOf` gives an equivalent query back. */
export function decompose(query: string): Choices {
  const f: Choices = { view: "all", kinds: [], interests: [], sources: [], segment: "all", query: "" };
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
    } else if (free && field === "kind" && lower.every(isKind)) {
      const kinds = [...new Set(lower)] as ItemKind[];
      const segment = SEGMENTS.find((s) => s.kinds.length === kinds.length && s.kinds.every((k) => kinds.includes(k)));
      if (segment) f.segment = segment.segment;
      else [f.kinds, f.segment] = [kinds, segmentOf(kinds)];
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

/** A saved search applied: its choices, text and order in place of the tab's (the groups unfolded stay). */
export const applied = (f: FeedFilters, search: { query: string; sort: Sort }): FeedFilters => ({
  ...f,
  ...decompose(search.query),
  sort: search.sort,
});

/** Whether a saved search is what the tab shows: the same query, the same order. */
export const isShown = (f: FeedFilters, search: { query: string; sort: Sort }) =>
  search.sort === f.sort && sameQuery(search.query, queryOf(f));

/** A choice shown as a chip above the list; the × takes it off. */
export type FeedChip = { kind: "view" | "kind" | "interest" | "source" | "search"; text: string };

/** A chip of several values: `Source: TC, GRL`; one value alone. */
const several = (name: string, values: string[]) => (values.length > 1 ? `${name}: ${values.join(", ")}` : (values[0] ?? ""));

/** The chips of the choices made, in the order view › kinds › interests › sources › search. */
export function feedChips(f: FeedFilters): FeedChip[] {
  const list: FeedChip[] = [];
  const kindName = (kind: ItemKind) => KINDS.find((k) => k.kind === kind)?.name ?? kind;
  if (f.view !== "all") list.push({ kind: "view", text: SHELVES.find((s) => s.view === f.view)?.name ?? f.view });
  if (f.kinds.length) list.push({ kind: "kind", text: several("Kind", f.kinds.map(kindName)) });
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

/** Whether a journal is followed: a sweep of one of its ISSNs. */
export function journalFollowed(journal: Journal, subscriptions: SubscriptionInfo[]): boolean {
  return journal.issns.some((issn) => subscriptions.some((s) => sameFeed(s.feed, { crossref: issn })));
}

/** A journal of a search, in a line: "Cambridge University Press · ISSN 0022-1430 · 6886 works". */
export function journalFacts(journal: Journal): string {
  const facts = [
    journal.publisher,
    journal.issn && `ISSN ${journal.issn}`,
    journal.works !== null && `${journal.works} works`,
  ];
  return facts.filter(Boolean).join(" · ");
}

/** What subscribing to a journal comes to, from what its site gave: the journal's feed; else why not, and its home
 *  page to open (the user finds the feed there), for the choice of following it in Crossref instead, or of a feed
 *  that may not be the journal's (`other`). */
export type JournalChoice =
  | { feed: string }
  | { reason: string; homepage: string | null; other: { url: string; title: string | null } | null };

export function journalChoice(site: JournalSite): JournalChoice {
  const not = (reason: string, other: { url: string; title: string | null } | null = null) => ({
    reason,
    homepage: site.homepage,
    other,
  });
  if (!site.feed) return { reason: "No site known", homepage: null, other: null };
  const feed = site.feed;
  switch (feed.site) {
    case "found":
      return { feed: feed.url };
    case "other":
      return not(`A feed that may not be the journal's: ${feed.title ?? feed.url}`, {
        url: feed.url,
        title: feed.title,
      });
    case "refused":
      return not(`Its site refused: ${feed.reason}`);
    case "feedrefused":
      return not(`Its feed refused: ${feed.reason}`);
    case "notafeed":
      return not("Its site links to a feed that is none");
    case "nofeed":
      return not("No feed on its site's page");
  }
}

/** The feed a journal's row shows followed when it is: the journal's feed, or the other one offered. */
export function choiceFeed(choice: JournalChoice | { reading: true } | undefined): string | null {
  if (!choice || "reading" in choice) return null;
  return "feed" in choice ? choice.feed : (choice.other?.url ?? null);
}

/** The colours of the interests, in the order of their names. */
const COLOURS = ["#2f9e6e", "#d9822b", "#8a5cd6", "#c2410c", "#0e7490", "#a16207"];

/** The interests by name, each with its colour: the sidebar's dot, the badge of a row. */
export function interestColours(names: string[]): Map<string, string> {
  return new Map([...names].sort(collator.compare).map((name, i) => [name, COLOURS[i % COLOURS.length]]));
}
