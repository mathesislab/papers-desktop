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
  /** snoozed (out of Unread till `remind`), or due (new again, first in the list, not seen by being shown) */
  reminder: Reminder | null;
  remind: string | null;
  /** the ids of the library entries of the same paper (DOI, or title and year) */
  inLibrary: string[];
  /** a preprint, or a paper whose preprint is known */
  hasVersions: boolean;
  /** the posts that link to it */
  coveredBy: number;
  /** a post: the papers found behind it */
  links: number;
  /** the works that cite it, as OpenAlex counted them when it was found */
  cited: number | null;
}

/** The counts of the sidebar: over every item not dismissed. */
export interface FeedCounts {
  all: number;
  /** by `is:` value: new, seen, shortlisted, dismissed, added, editorial, inlibrary */
  is: Record<string, number>;
  total: FeedGroups;
  /** those new */
  unread: FeedGroups;
  /** what each saved search shows, by its name */
  searches: Record<string, number>;
}

/** Items by kind, interest and subscription. */
export interface FeedGroups {
  kinds: Partial<Record<ItemKind, number>>;
  interests: Record<string, number>;
  /** by subscription title */
  sources: Record<string, number>;
}

export interface FeedView {
  rows: FeedRow[];
  counts: FeedCounts;
  /** the items fetched since the list was shown, held back */
  held: number;
}

export interface ViewRequest {
  query: string;
  sort: Sort;
  /** when the list was last brought up to date (ISO): what was fetched since is held back; none: every item */
  shown?: string | null;
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
  /** the abstract is the text of a feed (a teaser, a summary), not Crossref's or OpenAlex's */
  abstractFromFeed: boolean;
  /** its lookups may still find its abstract: `lookUp` asks */
  abstractPending: boolean;
  /** a post's HTML as the feed gives it, made safe (no scripts, styles or frames): its text, links and images */
  content: string | null;
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
  /** the works that cite it, as OpenAlex counted them when it was found */
  cited: number | null;
  /** its OpenAlex id (`W…`): the works around it are searched by it, else by its DOI */
  openalex: string | null;
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

/** Whether the main window shows the Feeds tab (the setting `feeds_tab`). */
export const tabShown = () => invoke<boolean>("feeds_tab");
/** Shows or hides the Feeds tab: the windows are told (`EVENTS.tab`), and the menu follows. */
export const setTab = (show: boolean) => invoke<void>("feeds_set_tab", { show });

/** Whether the Feeds tab shows an item's details beside the list (the setting `feeds_beside`), rather than in place
 *  of it, to read. */
export const besideShown = () => invoke<boolean>("feeds_beside");
/** The details beside the list, or in place of it: the windows are told (`EVENTS.beside`), and the menu follows. */
export const setBeside = (beside: boolean) => invoke<void>("feeds_set_beside", { beside });

/** A saved search of the sidebar: the whole query of the tab's choices (filters.ts, `queryOf`) and the order. */
export interface SavedSearch {
  name: string;
  query: string;
  sort: Sort;
}

/** A subscription offered in the settings and the Subscribe sheet. */
export interface Suggested {
  title: string;
  kind: SourceKind;
  feed: Feed;
  /** a group of the list: "Copernicus", "Nature", "AGU", "News", ... */
  group: string;
}

/** A journal, as OpenAlex knows it, or as the library's papers give its ISSN. */
export interface Journal {
  title: string;
  /** the ISSN its works are swept by in Crossref */
  issn: string | null;
  /** every ISSN of it, print and electronic */
  issns: string[];
  publisher: string | null;
  homepage: string | null;
  /** its works in OpenAlex */
  works: number | null;
}

/** What follows a journal of the library: a suggested subscription, a sweep of its ISSN (none if none was found),
 *  nothing found in OpenAlex, or not looked up yet. */
export type Follow =
  | { by: "suggested"; suggested: Suggested }
  | { by: "journal"; journal: Journal }
  | { by: "unknown" }
  | { by: "pending" };

/** What a journal's home page links to, its feeds read: the journal's feed (its link or its title names the
 *  journal); a feed that may not be the journal's (none names it); or its page or its feed refused (an error status,
 *  no answer), what it links to is no feed, or it links to none. */
export type SiteFeed =
  | { site: "found"; url: string; title: string | null }
  | { site: "other"; url: string; title: string | null }
  | { site: "refused"; reason: string }
  | { site: "feedrefused"; url: string; reason: string }
  | { site: "notafeed"; url: string }
  | { site: "nofeed" };

/** A journal's home page (OpenAlex's, else that of its ISSN), and what it links to; none without a home page. */
export interface JournalSite {
  homepage: string | null;
  feed: SiteFeed | null;
}

/** A journal of the library's papers (3 or more), and what follows it. */
export interface LibraryJournal {
  title: string;
  papers: number;
  follow: Follow;
}

/** Every subscription fetched, or the one of id `only`: the summary once the feeds are read; the lookups that follow
 *  go on (`EVENTS.fetching`). */
export const fetchFeeds = (only?: string) => invoke<FetchSummary>("feeds_fetch", { only: only ?? null });
export const lastFetch = () => invoke<FetchSummary | null>("feeds_last_fetch");
export const view = (request: ViewRequest) => invoke<FeedView>("feeds_view", { request });
/** The folder of feeds an item is of: the Feeds tab's, or the works found in the Search tab. */
export type Store = "feeds" | "search";

export const detail = (id: string, query: string, store: Store = "feeds") =>
  invoke<FeedDetail>("feeds_item", { store, id, query });
export const setTriage = (ids: string[], triage: Triage, store: Store = "feeds") =>
  invoke<void>("feeds_triage", { store, ids, triage });
export const findPapers = (id: string) => invoke<Linked[]>("feeds_find_papers", { id });
/** A kind guessed for a feed, and what tells it. */
export interface Guess {
  kind: SourceKind;
  reason: string;
}
/** The kind of the feed at an address, guessed from its items (DOIs, preprints) and its title (OpenAlex). */
export const guessKind = (url: string) => invoke<Guess>("feeds_guess_kind", { url });

/** Where an item to be reminded of is. */
export type Reminder = "snoozed" | "due";
/** Items new again, to be reminded of `until` (snoozed till then), else now (kept unread). */
export const remind = (ids: string[], until: string | null) => invoke<void>("feeds_remind", { ids, until });

/** The abstract of an item looked up now; whether it asked (the feeds then change). */
export const lookUp = (id: string, store: Store = "feeds") => invoke<boolean>("feeds_look_up", { store, id });
const pdfStatus = (id: string, store: Store) => invoke<PdfStatus>("feeds_pdf", { store, id });

/** the free PDFs asked for, by store and item */
const pdfs = new Map<string, Promise<PdfStatus>>();

/** The free PDF of an item, asked once a session (again after a failure). */
export function pdfOf(id: string, store: Store = "feeds"): Promise<PdfStatus> {
  const key = `${store} ${id}`;
  let found = pdfs.get(key);
  if (!found) {
    found = pdfStatus(id, store);
    found.catch(() => pdfs.delete(key));
    pdfs.set(key, found);
  }
  return found;
}

export const subscriptions = () => invoke<SubscriptionInfo[]>("feeds_subscriptions");
/** With a title (else the feed's own, at the next fetch). */
export const subscribe = (feed: Feed, kind: SourceKind, title?: string) =>
  invoke<SubscriptionInfo>("feeds_subscribe", { feed, kind, title: title ?? null });
export const unsubscribe = (id: string) => invoke<void>("feeds_unsubscribe", { id });
export const updateSubscription = (id: string, patch: SubscriptionPatch) =>
  invoke<SubscriptionInfo>("feeds_update_subscription", { id, patch });
export const suggested = () => invoke<Suggested[]>("feeds_suggested");
/** At once: those to look up are pending, for `lookupJournal`. */
export const librarySuggested = () => invoke<LibraryJournal[]>("feeds_library_suggested");
/** Looked up in OpenAlex by its title, once: what it answers is kept. */
export const lookupJournal = (title: string) => invoke<Follow>("feeds_lookup_journal", { title });
/** OpenAlex's journals of a text, the best first, 10 at most. */
export const searchJournals = (text: string) => invoke<Journal[]>("feeds_search_journals", { text });
/** Its home page read, for the feed it links to. */
export const journalFeed = (journal: Journal) => invoke<JournalSite>("feeds_journal_feed", { journal });
export const importOpml = (path: string) => invoke<number>("feeds_import_opml", { path });
export const exportOpml = (path: string) => invoke<void>("feeds_export_opml", { path });

export const settings = () => invoke<FeedSettings>("feeds_settings");
/** Refused, with the message of the first interest or rule that does not parse. */
export const setSettings = (settings: FeedSettings) =>
  invoke<void>("feeds_set_settings", { settings });

export const savedSearches = () => invoke<SavedSearch[]>("feeds_searches");
/** Refused for a blank name, or one given twice. */
export const setSavedSearches = (searches: SavedSearch[]) => invoke<void>("feeds_set_searches", { searches });

/** Where a fetch is: reading the feeds, looking up what they brought (Crossref, OpenAlex), done. */
export type FetchPhase = "reading" | "looking-up" | "done";

/** The events the page listens to. */
export const EVENTS = {
  /** the items changed: a fetch, a triage from elsewhere, papers found, the library indexed again */
  changed: "feeds-changed",
  /** where a fetch is (`FetchPhase`) */
  fetching: "feeds-fetching",
  /** the Feeds tab shown (true) or hidden (false) */
  tab: "feeds-tab",
  /** the details beside the list (true) or in place of it (false) */
  beside: "feeds-beside",
} as const;
