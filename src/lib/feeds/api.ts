// The commands of the Feeds tab and of the Feeds section of the settings
// (src-tauri/src/feeds.rs): their arguments and answers, as the page sees
// them. Dates are ISO strings; ids are the items' of crates/feeds.

import { invoke } from "@tauri-apps/api/core";
import type { Span } from "$lib/types";

export type ItemKind = "paper" | "preprint" | "news" | "blog";
export type SourceKind = "journal" | "preprints" | "news" | "blog";
export type Triage = "new" | "seen" | "shortlisted" | "dismissed" | "added";
export type Sort = "score" | "newest" | "like";

/** What a subscription follows: a feed, or a sweep of Crossref or OpenAlex. */
export type Feed = { rss: string } | { crossref: string } | { openalex: string };

/** An interest an item is scored in. */
export interface Scored {
  interest: string;
  score: number;
  /** the labels of the terms that matched */
  terms: string[];
  /** the interest's `min_score` or more, not vetoed */
  reached: boolean;
}

/** A row of the list. */
export interface FeedRow {
  id: string;
  kind: ItemKind;
  title: Span[];
  /** the family names, each marked where the query matched */
  authors: Span[][];
  journal: string | null;
  /** the titles of the subscriptions that listed it */
  sources: string[];
  date: string | null;
  /** the best score of its interests, 0 without */
  score: number;
  interests: Scored[];
  /** how like the library's papers it is, 0–100; null before the library is indexed */
  like: number | null;
  triage: Triage;
  /** the ids of the library entries of the same paper (DOI, or title and year) */
  inLibrary: string[];
  /** a preprint, or a paper whose preprint is known */
  hasVersions: boolean;
  /** the posts that link to it */
  coveredBy: number;
  /** a post: the papers found behind it */
  links: number;
}

/** The counts of the sidebar: over every item not dismissed. */
export interface FeedCounts {
  all: number;
  /** by `is:` value: new, seen, shortlisted, dismissed, added, editorial, inlibrary */
  is: Record<string, number>;
  kinds: Partial<Record<ItemKind, number>>;
  interests: Record<string, number>;
  /** by subscription title */
  sources: Record<string, number>;
}

export interface FeedView {
  rows: FeedRow[];
  counts: FeedCounts;
}

export interface ViewRequest {
  query: string;
  sort: Sort;
  /** none: every kind */
  kinds: ItemKind[];
}

/** A paper behind a post. */
export interface Linked {
  doi: string;
  confidence: "high" | "low";
  via: "feed" | "page" | "search";
  /** the item of that paper, when there is one */
  item: string | null;
  title: string | null;
}

/** An item, for the details pane and the review. */
export interface FeedDetail {
  id: string;
  kind: ItemKind;
  title: Span[];
  authors: string[];
  journal: string | null;
  date: string | null;
  doi: string | null;
  arxiv: string | null;
  url: string | null;
  /** the abstract (a post: its summary), marked where the query or an interest matched */
  abstract: Span[];
  /** the abstract came from Crossref or OpenAlex, not the feed */
  abstractLookedUp: boolean;
  /** a post's text as the feed gives it, as plain paragraphs */
  content: string[];
  editorial: boolean;
  sources: { id: string; title: string; kind: SourceKind }[];
  versions: { doi: string; stage: "preprint" | "published"; item: string | null }[];
  links: Linked[];
  coveredBy: { id: string; title: string; source: string }[];
  interests: Scored[];
  triage: Triage;
  inLibrary: string[];
  /** library entries of another version of it (its preprint, say) */
  versionInLibrary: string[];
  /** how like the library's papers it is, 0–100; null before the library is indexed */
  like: number | null;
  /** the library's papers most like it, the nearest first (3 at most) */
  nearest: Nearest[];
}

/** A paper of the library like an item of the feeds. */
export interface Nearest {
  /** the library entry's id */
  id: string;
  key: string;
  title: string;
  /** "Feldmann & Levermann · 2017 · Cryosphere" */
  subtitle: string;
  /** of the item's words and the paper's (TF-IDF), 0–1 */
  cosine: number;
}

/** What the open access copy of a paper is, as OpenAlex knows it. */
export interface PdfStatus {
  /** "open": a free PDF; "preprint": a free copy of another version; "closed" */
  access: "open" | "preprint" | "closed" | "unknown";
  url: string | null;
  licence: string | null;
  host: string | null;
}

export interface FetchSummary {
  at: string;
  fetched: number;
  unchanged: number;
  failed: { subscription: string; title: string; message: string }[];
  /** items new in this fetch */
  added: number;
  lookups: number;
  pages: number;
}

export interface SubscriptionInfo {
  id: string;
  title: string;
  kind: SourceKind;
  feed: Feed;
  journal: string | null;
  fetchPages: boolean;
  /** added to the score of its items in each interest */
  boost: number;
  /** the last fetch of it: when, and its error if it failed */
  last: { at: string; error: string | null } | null;
}

export interface SubscriptionPatch {
  title?: string;
  kind?: SourceKind;
  journal?: string | null;
  fetchPages?: boolean;
  boost?: number;
}

export interface Weighted {
  term?: string;
  pattern?: string;
  weight: number;
  label?: string;
}

export interface Interest {
  name: string;
  include: Weighted[];
  exclude: { term?: string; pattern?: string }[];
  min_score: number;
}

export interface Rule {
  when: string;
  then: "shortlist" | "dismiss" | "seen";
}

export interface FeedSettings {
  interests: Interest[];
  titleBoost: number;
  rules: Rule[];
  keepDays: number;
  lookbackDays: number;
  /** fetch in the background, every `pollMinutes` */
  poll: boolean;
  pollMinutes: number;
}

/** A subscription offered in the settings and the Subscribe sheet. */
export interface Suggested {
  title: string;
  kind: SourceKind;
  feed: Feed;
  /** a group of the list: "Copernicus", "Nature", "AGU", "News", ... */
  group: string;
}

export const fetchFeeds = () => invoke<FetchSummary>("feeds_fetch");
export const lastFetch = () => invoke<FetchSummary | null>("feeds_last_fetch");
export const view = (request: ViewRequest) => invoke<FeedView>("feeds_view", { request });
export const detail = (id: string, query: string) =>
  invoke<FeedDetail>("feeds_item", { id, query });
export const setTriage = (ids: string[], triage: Triage) =>
  invoke<void>("feeds_triage", { ids, triage });
export const findPapers = (id: string) => invoke<Linked[]>("feeds_find_papers", { id });
export const pdfStatus = (id: string) => invoke<PdfStatus>("feeds_pdf", { id });

export const subscriptions = () => invoke<SubscriptionInfo[]>("feeds_subscriptions");
export const subscribe = (feed: Feed, kind: SourceKind) =>
  invoke<SubscriptionInfo>("feeds_subscribe", { feed, kind });
export const unsubscribe = (id: string) => invoke<void>("feeds_unsubscribe", { id });
export const updateSubscription = (id: string, patch: SubscriptionPatch) =>
  invoke<SubscriptionInfo>("feeds_update_subscription", { id, patch });
export const suggested = () => invoke<Suggested[]>("feeds_suggested");
export const importOpml = (path: string) => invoke<number>("feeds_import_opml", { path });
export const exportOpml = (path: string) => invoke<void>("feeds_export_opml", { path });

export const settings = () => invoke<FeedSettings>("feeds_settings");
/** Refused, with the message of the first interest or rule that does not parse. */
export const setSettings = (settings: FeedSettings) =>
  invoke<void>("feeds_set_settings", { settings });

/** The events the page listens to. */
export const EVENTS = {
  /** the items changed: a fetch, a triage from elsewhere, papers found, the library indexed again */
  changed: "feeds-changed",
  /** a fetch started (true) or ended (false) */
  fetching: "feeds-fetching",
} as const;
