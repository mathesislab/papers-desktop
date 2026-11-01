// The subscriptions of the Feeds tab: in their groups (the sidebar's sources, the Subscribe sheet's list), what is
// typed to subscribe and whether it is followed already, a feed said in a line, what subscribing to a journal comes to.
// The Subscribe sheet and the settings subscribe by them alike. Pure functions, tested by scripts/feeds.test.ts
// (npm test).

import type { Feed, FeedCounts, Journal, JournalSite, SourceKind, SubscriptionInfo } from "./api.ts";
import { SOURCE_KINDS } from "./kinds.ts";

/** A group of the sources in the sidebar: the subscriptions of a kind, the sweeps of Crossref and OpenAlex apart. */
export type SourceGroup = {
  group: SourceKind | "sweeps";
  name: string;
  unread: number;
  total: number;
  sources: { title: string; unread: number; total: number }[];
};

export const SOURCE_GROUPS: { group: SourceGroup["group"]; name: string }[] = [
  { group: "journal", name: SOURCE_KINDS.journal.plural },
  { group: "preprints", name: SOURCE_KINDS.preprints.plural },
  { group: "sweeps", name: "Sweeps (Crossref, OpenAlex)" },
  { group: "news", name: SOURCE_KINDS.news.plural },
  { group: "blog", name: SOURCE_KINDS.blog.plural },
];

const collator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });

/** The group of a subscription: its kind, or a sweep. */
export const groupOf = (s: SubscriptionInfo): SourceGroup["group"] => ("rss" in s.feed ? s.kind : "sweeps");

/** The subscriptions in their groups, by title, each with the number of its items and of those unread (`counts`);
 *  the groups without any are left out. */
export function sourceGroups(subscriptions: SubscriptionInfo[], counts: FeedCounts | null): SourceGroup[] {
  const sum = (sources: { unread: number; total: number }[], key: "unread" | "total") =>
    sources.reduce((n, s) => n + s[key], 0);
  return SOURCE_GROUPS.map(({ group, name }) => {
    const titles = [...new Set(subscriptions.filter((s) => groupOf(s) === group).map((s) => s.title))].sort(collator.compare);
    const sources = titles.map((title) => ({
      title,
      unread: counts?.unread.sources[title] ?? 0,
      total: counts?.total.sources[title] ?? 0,
    }));
    return { group, name, sources, unread: sum(sources, "unread"), total: sum(sources, "total") };
  }).filter((g) => g.sources.length > 0);
}

/** What a subscription follows, as the Subscribe sheet asks it: a feed's URL, a journal's ISSN for Crossref, a
 *  filter of OpenAlex. */
export type FeedType = "rss" | "crossref" | "openalex" | "words";

/** The feed of what was typed; none for an empty field. Words are an OpenAlex filter (`wordsFilter`). */
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
    case "words":
      return { openalex: wordsFilter(value) };
  }
}

/** The OpenAlex filter of the works whose title or abstract has words (with OpenAlex's AND, OR, NOT, quotes and
 *  parentheses): `title_and_abstract.search:` and the words, their commas (which would end the filter) and runs of
 *  white space as one space. */
export function wordsFilter(words: string): string {
  return `title_and_abstract.search:${words.replace(/[,\s]+/g, " ").trim()}`;
}

/** A feed in a line: its URL, the ISSN swept in Crossref, the filter of OpenAlex. */
export const feedText = (feed: Feed) =>
  "rss" in feed ? feed.rss : "crossref" in feed ? `Crossref ISSN ${feed.crossref}` : `OpenAlex ${feed.openalex}`;

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

/** The subscription of a feed, if there is one. */
export const subscriptionOf = (feed: Feed, subscriptions: SubscriptionInfo[]) =>
  subscriptions.find((s) => sameFeed(s.feed, feed));

/** The subscription that follows a journal by a sweep of one of its ISSNs, if there is one. */
export function journalSubscription(journal: Journal, subscriptions: SubscriptionInfo[]): SubscriptionInfo | undefined {
  for (const issn of journal.issns) {
    const found = subscriptionOf({ crossref: issn }, subscriptions);
    if (found) return found;
  }
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
