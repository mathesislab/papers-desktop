// The Feeds tab: its filters, the triage of the keys, the list kept in place, seen on dwell.
// node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { mock, test } from "node:test";
import type { FeedCounts, FeedRow, Journal, SubscriptionInfo, Triage } from "../src/lib/feeds/api.ts";
import {
  NO_FEED_FILTERS,
  chooseInterest,
  chooseKind,
  chooseSource,
  feedChips,
  nextSort,
  queryOf,
  requestOf,
  restoredFeeds,
  shelfCounts,
  withoutChip,
  type FeedFilters,
} from "../src/lib/feeds/filters.ts";
import {
  SOURCE_GROUPS,
  choiceFeed,
  feedOf,
  feedText,
  journalChoice,
  journalFacts,
  journalSubscription,
  sameFeed,
  sourceGroups,
  wordsFilter,
} from "../src/lib/feeds/subscriptions.ts";
import {
  addTarget,
  Dwell,
  ago,
  badges,
  barOf,
  fetchLine,
  mergeRows,
  NO_INTERESTS,
  newAbove,
  newOlderThan,
  scoreHint,
  since,
  snoozeUntil,
  stale,
  toggled,
} from "../src/lib/feeds/items.ts";
import { ITEM_KINDS, SOURCE_KINDS, isItemKind, isPost, itemKinds, sourceKinds } from "../src/lib/feeds/kinds.ts";

const f = (over: Partial<FeedFilters> = {}): FeedFilters => ({ ...NO_FEED_FILTERS, ...over });

test("the query: a term per group of choices, then the text typed", () => {
  assert.equal(queryOf(f()), "is:new", "every kind");
  assert.equal(queryOf(f({ view: "all" })), "");
  assert.equal(
    queryOf(f({ query: " greenland ", view: "starred", kinds: ["preprint"], interests: ["Ice sheets"], sources: ["The Cryosphere", "GRL"] })),
    'is:starred kind:preprint prof:"Ice sheets" src:"The Cryosphere",GRL greenland',
  );
  assert.equal(queryOf(f({ kinds: ["news", "blog"], interests: ['say "hi"'] })), 'is:new kind:news,blog prof:"say hi"');
  assert.deepEqual(requestOf(f({ kinds: ["news"], sort: "newest" })), { query: "is:new kind:news", sort: "newest" });
});

test("a click chooses one of a group, ⌘-click several", () => {
  assert.deepEqual(chooseKind(f(), "news"), f({ kinds: ["news"] }));
  assert.deepEqual(chooseKind(f({ kinds: ["news"] }), "news").kinds, []);
  assert.deepEqual(chooseKind(f({ kinds: ["news", "blog"] }), "news").kinds, ["news"]);
  // ⌘: added, or taken out
  assert.deepEqual(chooseKind(f({ kinds: ["paper"] }), "news", true), f({ kinds: ["paper", "news"] }));
  assert.deepEqual(chooseKind(f({ kinds: ["paper", "news"] }), "paper", true).kinds, ["news"]);
  assert.deepEqual(chooseInterest(chooseInterest(f(), "AMOC"), "AMOC").interests, []);
  assert.deepEqual(chooseInterest(f({ interests: ["AMOC", "Ice"] }), "AMOC").interests, ["AMOC"]);
  assert.deepEqual(chooseInterest(f({ interests: ["AMOC"] }), "Ice", true).interests, ["AMOC", "Ice"]);
  assert.deepEqual(chooseSource(f({ sources: ["TC", "GRL"] }), "TC", true).sources, ["GRL"]);
  assert.deepEqual(chooseSource(f({ sources: ["TC"] }), "GRL").sources, ["GRL"]);
});

test("chips, a chip per group, and their ×", () => {
  const all = f({ view: "unread", kinds: ["paper"], interests: ["AMOC"], sources: ["GRL"], query: "melt" });
  assert.deepEqual(
    feedChips(all).map((c) => c.text),
    ["Unread", "Papers", "AMOC", "GRL", '"melt"'],
  );
  assert.deepEqual(feedChips(f({ view: "all" })), []);
  const [view, kind, interest, source, search] = feedChips(all);
  assert.equal(withoutChip(all, view).view, "all");
  assert.deepEqual(withoutChip(all, kind).kinds, []);
  assert.deepEqual(withoutChip(all, interest).interests, []);
  assert.deepEqual(withoutChip(all, source).sources, []);
  assert.equal(withoutChip(all, search).query, "");
  const many = f({ view: "all", kinds: ["paper", "preprint"], interests: ["A", "B"], sources: ["TC", "GRL"] });
  assert.deepEqual(
    feedChips(many).map((c) => c.text),
    ["Kind: Papers, Preprints", "Interest: A, B", "Source: TC, GRL"],
  );
});

test("restored: what is missing or wrong takes the default; a single choice of before, a group of one", () => {
  assert.deepEqual(restoredFeeds(null), NO_FEED_FILTERS);
  assert.deepEqual(restoredFeeds({ view: "nope" as never, sort: "newest", open: ["news", 3 as never] }), {
    ...NO_FEED_FILTERS,
    sort: "newest",
    open: ["news"],
  });
  // the segment of before is let go: every kind
  const before = { kind: "news", interest: "AMOC", source: "Eos", segment: "posts" };
  assert.deepEqual(restoredFeeds(before), { ...NO_FEED_FILTERS, kinds: ["news"], interests: ["AMOC"], sources: ["Eos"] });
  assert.deepEqual(restoredFeeds({ segment: "papers" } as never), NO_FEED_FILTERS);
  assert.deepEqual(restoredFeeds({ kind: null, interest: null, source: null }), NO_FEED_FILTERS);
  assert.deepEqual(restoredFeeds({ kinds: ["blog", "nope" as never, "blog"], sources: ["A", 2 as never] }), {
    ...NO_FEED_FILTERS,
    kinds: ["blog"],
    sources: ["A"],
  });
  assert.equal(restoredFeeds({ sort: "like" as never }).sort, "score", "the sort by likeness of before: the score");
  assert.equal(restoredFeeds({ sort: "oldest" as never }).sort, "score");
});

test("the sorts in turn", () => {
  assert.deepEqual([nextSort("score"), nextSort("newest")], ["newest", "score"]);
  assert.deepEqual(requestOf(f({ sort: "newest" })).sort, "newest");
});

test("a score says its parts on hover, the exclude that vetoed it, or why it has no keywords", () => {
  const scored = (interest: string, terms: string[]) => ({ interest, score: 6, scaled: 100, terms, excluded: null, reached: true });
  const rated = { score: 72.4, keywords: 72.4, library: 41.2, excluded: null };
  const interests = [scored("Ice", ["ice sheet", "GIA"]), scored("AMOC", ["AMOC", "GIA"])];
  assert.equal(scoreHint({ ...rated, interests }), "Score 72 · keywords 72 (ice sheet, GIA, AMOC) · library 41");
  assert.equal(scoreHint({ ...rated, keywords: 0, interests: [] }), "Score 72 · keywords 0 · library 41");
  assert.equal(scoreHint({ ...rated, keywords: null, library: null, interests: [] }), `Score 72\n${NO_INTERESTS}`);
  assert.equal(scoreHint({ score: 0, keywords: 0, library: 0, excluded: "Georgia", interests }), "Excluded: Georgia");
});

test("a part of the score fills its bar from 0 to 100", () => {
  assert.deepEqual([barOf(null), barOf(0), barOf(41.2), barOf(100), barOf(-3), barOf(140)], [0, 0, 41.2, 100, 0, 100]);
});

test("the counts of the views, and the sources in their groups", () => {
  assert.deepEqual(shelfCounts(null), { unread: 0, starred: 0, snoozed: 0, added: 0, all: 0 });
  assert.deepEqual(shelfCounts({ all: 9, is: { new: 4, starred: 2, snoozed: 1 }, kinds: {}, interests: {}, sources: {} }), {
    unread: 4,
    starred: 2,
    snoozed: 1,
    added: 0,
    all: 9,
  });
  const sub = (title: string, kind: SubscriptionInfo["kind"], feed: SubscriptionInfo["feed"]): SubscriptionInfo => ({
    id: title,
    title,
    kind,
    feed,
    journal: null,
    fetchPages: false,
    boost: 0,
    last: null,
  });
  const groups = sourceGroups(
    [
      sub("TC", "journal", { url: "x" }),
      sub("GRL", "journal", { url: "y" }),
      sub("Ice", "journal", { openalex: "z" }),
      sub("Eos", "news", { url: "e" }),
    ],
    {
      total: { kinds: {}, interests: {}, sources: { TC: 3, GRL: 1, Ice: 5 } },
      unread: { kinds: {}, interests: {}, sources: { TC: 2, Ice: 5 } },
    } as FeedCounts,
  );
  assert.deepEqual(
    groups.map((g) => [g.group, g.unread, g.total, g.sources.map((s) => s.title)]),
    [
      ["journal", 2, 4, ["GRL", "TC"]],
      ["sweeps", 5, 5, ["Ice"]],
      ["news", 0, 0, ["Eos"]],
    ],
  );
  assert.deepEqual(groups[0].sources[1], { title: "TC", unread: 2, total: 3 });
});

test("the feed of the Subscribe sheet, and the same feed", () => {
  assert.deepEqual(feedOf("url", " https://x.org/feed "), { url: "https://x.org/feed" });
  assert.deepEqual(feedOf("issn", "1994-0424"), { issn: "1994-0424" });
  assert.equal(feedOf("openalex", "  "), null);
  assert.deepEqual(feedOf("words", " ice sheet AND (tipping OR collapse) "), {
    openalex: "title_and_abstract.search:ice sheet AND (tipping OR collapse)",
  });
  assert.equal(wordsFilter("AMOC,  collapse,\ttipping"), "title_and_abstract.search:AMOC collapse tipping");
  assert.ok(sameFeed({ url: "https://x.org/feed/" }, { url: "http://X.org/feed" }));
  assert.ok(sameFeed({ issn: "1994-0424" }, { issn: "19940424" }));
  assert.ok(!sameFeed({ url: "x" }, { issn: "x" }));
  assert.equal(feedText({ issn: "1994-0424" }), "Crossref ISSN 1994-0424");
  assert.equal(feedText({ url: "https://x.org/feed" }), "https://x.org/feed");
});

const glaciology: Journal = {
  title: "Journal of Glaciology",
  issn: "0022-1430",
  issns: ["0022-1430", "1727-5652"],
  publisher: "Cambridge University Press",
  homepage: "https://www.cambridge.org/core/journals/journal-of-glaciology",
  works: 6886,
};

test("a journal of the Subscribe sheet: its facts, followed or not, what its site gives", () => {
  assert.equal(journalFacts(glaciology), "Cambridge University Press · ISSN 0022-1430 · 6886 works");
  assert.equal(journalFacts({ ...glaciology, publisher: null, works: null }), "ISSN 0022-1430");
  const sweep = (issn: string) => ({ feed: { issn } }) as SubscriptionInfo;
  assert.deepEqual(journalSubscription(glaciology, [sweep("17275652")])?.feed, { issn: "17275652" }, "by any of its ISSNs");
  assert.equal(journalSubscription(glaciology, [sweep("1994-0424")]), undefined);
  const homepage = glaciology.homepage;
  const url = "https://j.example.org/rss";
  const found = journalChoice({ homepage, feed: { site: "found", url, title: null } });
  assert.deepEqual(found, { feed: url });
  assert.equal(choiceFeed(found), url);
  assert.deepEqual(journalChoice({ homepage, feed: { site: "refused", reason: "HTTP 403" } }), {
    reason: "Its site refused: HTTP 403",
    homepage,
    other: null,
  });
  assert.equal(
    (journalChoice({ homepage, feed: { site: "feedrefused", url, reason: "HTTP 403" } }) as { reason: string }).reason,
    "Its feed refused: HTTP 403",
  );
  const other = journalChoice({ homepage, feed: { site: "other", url, title: "AMS News" } });
  assert.deepEqual(other, {
    reason: "A feed that may not be the journal's: AMS News",
    homepage,
    other: { url, title: "AMS News" },
  });
  assert.equal(choiceFeed(other), url);
  assert.deepEqual(journalChoice({ homepage, feed: { site: "nofeed" } }), {
    reason: "No feed on its site's page",
    homepage,
    other: null,
  });
  assert.equal(choiceFeed(journalChoice({ homepage, feed: { site: "nofeed" } })), null);
  assert.deepEqual(journalChoice({ homepage: null, feed: null }), {
    reason: "No site known",
    homepage: null,
    other: null,
  });
});

const it = (id: string, triage: Triage) => ({ id, triage });

test("the keys toggle the triage of the selection", () => {
  assert.deepEqual(toggled([it("a", "new"), it("b", "starred")], "star"), { ids: ["a"], triage: "starred" });
  assert.deepEqual(toggled([it("a", "starred"), it("b", "starred")], "star"), { ids: ["a", "b"], triage: "seen" });
  assert.deepEqual(toggled([it("a", "seen")], "remove"), { ids: ["a"], triage: "removed" });
  assert.deepEqual(toggled([it("a", "removed")], "remove"), { ids: ["a"], triage: "seen" });
  // m: the new ones seen; with none new, the seen ones new; the others keep theirs
  assert.deepEqual(toggled([it("a", "new"), it("b", "seen"), it("c", "added")], "read"), { ids: ["a"], triage: "seen" });
  assert.deepEqual(toggled([it("b", "seen"), it("c", "starred")], "read"), { ids: ["b"], triage: "new" });
  assert.equal(toggled([it("c", "added")], "read"), null);
  assert.equal(toggled([], "star"), null);
  // an item added keeps being added
  assert.deepEqual(toggled([it("a", "starred"), it("c", "added")], "star"), { ids: ["a"], triage: "seen" });
  assert.deepEqual(toggled([it("a", "new"), it("c", "added")], "star"), { ids: ["a"], triage: "starred" });
  assert.equal(toggled([it("c", "added")], "star"), null);
});

test("mark all above, ranges, the next after some go", () => {
  const rows = [it("a", "new"), it("b", "seen"), it("c", "new"), it("d", "new")];
  assert.deepEqual(newAbove(rows, "d"), ["a", "c"]);
  assert.deepEqual(newAbove(rows, "a"), []);
  assert.deepEqual(newAbove(rows, null), ["a", "c", "d"]);
  const dated = [
    { id: "a", triage: "new" as Triage, date: "2026-10-30T00:00:00Z" },
    { id: "b", triage: "new" as Triage, date: "2026-10-20T00:00:00Z" },
    { id: "c", triage: "seen" as Triage, date: "2026-10-20T00:00:00Z" },
    { id: "d", triage: "new" as Triage, date: null },
    { id: "e", triage: "new" as Triage, reminder: "snoozed" as const, date: "2026-10-20T00:00:00Z" },
  ];
  assert.deepEqual(newOlderThan(dated, 3, Date.parse("2026-10-31T00:00:00Z")), ["b"], "a snoozed item is not unread");
  assert.deepEqual(snoozeUntil(3, new Date(2026, 9, 31, 15, 30)), new Date(2026, 10, 3), "the start of the day");
});

const row = (id: string, triage: Triage = "new"): FeedRow => ({
  id,
  kind: "paper",
  title: [{ text: id, mark: false }],
  authors: [],
  journal: null,
  sources: [],
  date: null,
  score: 0,
  keywords: null,
  library: null,
  excluded: null,
  interests: [],
  triage,
  reminder: null,
  remind: null,
  inLibrary: [],
  hasVersions: false,
  coveredBy: 0,
  links: 0,
});

test("the list again: items triaged here stay in their place", () => {
  const ids = (rows: FeedRow[]) => rows.map((r) => r.id);
  const old = [row("a"), row("b"), row("c"), row("d")];
  // b and d were marked seen in Unread: the answer lacks them
  assert.deepEqual(ids(mergeRows(old, [row("a"), row("c"), row("e")], new Set(["b", "d"]))), ["a", "b", "c", "d", "e"]);
  assert.deepEqual(ids(mergeRows(old, [row("c")], new Set(["a"]))), ["a", "c"]);
  // not sticky: gone
  assert.deepEqual(ids(mergeRows(old, [row("a")], new Set())), ["a"]);
  // the answer's row wins
  assert.equal(mergeRows(old, [row("b", "seen")], new Set(["b"]))[0].triage, "seen");
});

test("what an item adds", () => {
  const d = { id: "i", kind: "paper" as const, doi: null, arxiv: null, links: [] };
  assert.deepEqual(addTarget({ ...d, doi: "10.1/x" }), { id: "10.1/x", item: "i" });
  assert.deepEqual(addTarget({ ...d, kind: "preprint", arxiv: "2410.01234v2" }), { id: "2410.01234v2", item: "i" });
  assert.equal(addTarget(d), null);
  const links = [
    { doi: "10.1/low", confidence: "low" as const, via: "search" as const, item: null, title: null },
    { doi: "10.1/high", confidence: "high" as const, via: "page" as const, item: "p", title: null },
  ];
  assert.deepEqual(addTarget({ ...d, kind: "news", links }), { id: "10.1/high", item: "p" });
  assert.equal(addTarget({ ...d, kind: "blog" }), null);
});

test("ages, staleness and the status line", () => {
  const now = Date.parse("2026-10-31T12:00:00Z");
  const at = (minutes: number) => new Date(now - minutes * 60000).toISOString();
  assert.equal(ago(at(0), now), "now");
  assert.equal(ago(at(8), now), "8 min");
  assert.equal(ago(at(150), now), "2 h");
  assert.equal(ago(at(3 * 24 * 60), now), "3 d");
  assert.equal(ago(null, now), "");
  assert.equal(since(at(0), now), "just now");
  assert.equal(since(at(150), now), "2 h ago");
  assert.match(since(at(30 * 24 * 60), now), /^on /);
  const summary = (minutes: number) => ({ at: at(minutes), fetched: 5, unchanged: 3, failed: [], added: 12, lookups: 0, pages: 0 });
  assert.equal(stale(null, now), true);
  assert.equal(stale(summary(10), now), false);
  assert.equal(stale(summary(16), now), true);
  assert.equal(fetchLine(summary(8), now), "fetched 8 min ago · 12 new · 3 not modified");
  assert.equal(
    fetchLine({ ...summary(0), added: 0, unchanged: 0, failed: [{ subscription: "s", title: "IGS", message: "403" }] }, now),
    "fetched just now · 1 failed",
  );
  assert.equal(fetchLine(null, now), "not fetched yet");
});

test("badges", () => {
  const r = {
    ...row("a"),
    inLibrary: ["x"],
    hasVersions: true,
    coveredBy: 2,
    interests: [
      { interest: "AMOC", score: 4, scaled: 66.7, terms: [], excluded: null, reached: true },
      { interest: "Ice", score: 1, scaled: 16.7, terms: [], excluded: null, reached: false },
    ],
  };
  const one = () => null;
  assert.deepEqual(
    badges(r, one).map((b) => b.text),
    ["in library", "has preprint", "2 news", "AMOC"],
  );
  assert.deepEqual(badges({ ...row("p"), kind: "news", links: 1 }, one).map((b) => b.text), ["→ paper"]);
  // a library of several .bib files: each named, once
  const bibs: Record<string, string> = { x: "thesis.bib", y: "papers.bib", z: "thesis.bib" };
  const several = (id: string) => bibs[id] ?? null;
  assert.deepEqual(
    badges({ ...row("b"), inLibrary: ["x", "y", "z"] }, several).map((b) => b.text),
    ["in thesis.bib", "in papers.bib"],
  );
});

test("seen on dwell: an item that stays, not one passed over", () => {
  mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const seen: string[] = [];
    const dwell = new Dwell((id) => seen.push(id), 800);
    dwell.show("a", true);
    mock.timers.tick(300);
    dwell.show("b", true);
    mock.timers.tick(300);
    dwell.show("c", false);
    mock.timers.tick(1000);
    assert.deepEqual(seen, []);
    dwell.show("d", true);
    mock.timers.tick(800);
    assert.deepEqual(seen, ["d"]);
    dwell.show("e", true);
    dwell.stop();
    mock.timers.tick(1000);
    assert.deepEqual(seen, ["d"]);
  } finally {
    mock.timers.reset();
  }
});

test("the kinds, named once", () => {
  assert.deepEqual(itemKinds, ["paper", "preprint", "news", "blog"]);
  assert.deepEqual(sourceKinds, ["journal", "preprints", "news", "blog"]);
  assert.equal(isItemKind("preprint"), true);
  assert.equal(isItemKind("preprints"), false);
  assert.equal(isItemKind("constructor"), false);
  assert.equal(isPost("news") && isPost("blog") && !isPost("paper") && !isPost("preprint"), true);
  // the sidebar's groups of sources take the kinds' plurals
  assert.deepEqual(
    SOURCE_GROUPS.map((g) => g.name),
    ["Journals", "Preprint servers", "Sweeps (Crossref, OpenAlex)", "News & press", "General"],
  );
  assert.equal(feedChips(f({ kinds: ["paper", "news"] }))[1].text, "Kind: Papers, News & press");
  assert.equal(ITEM_KINDS.blog.name, "general");
  assert.equal(SOURCE_KINDS.preprints.name, "Preprint server");
});
