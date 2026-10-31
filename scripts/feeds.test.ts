// The Feeds tab: its filters, the triage of the keys, the list kept in place, seen on dwell, the review.
// node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { mock, test } from "node:test";
import type { FeedRow, SubscriptionInfo, Triage } from "../src/lib/feeds/api.ts";
import {
  NO_FEED_FILTERS,
  chooseInterest,
  chooseKind,
  chooseSegment,
  facet,
  feedChips,
  feedOf,
  nextSort,
  requestOf,
  restoredFeeds,
  sameFeed,
  searchOf,
  shelfCounts,
  sourceGroups,
  withoutChip,
  type FeedFilters,
} from "../src/lib/feeds/filters.ts";
import {
  addField,
  addTarget,
  ago,
  arxivDoi,
  badges,
  fetchLine,
  likeText,
  mergeRows,
  newAbove,
  nextAfter,
  rangeOf,
  stale,
  toggled,
} from "../src/lib/feeds/items.ts";
import { Dwell, decide, progressOf, reviewed, startReview, stepReview, summaryOf } from "../src/lib/feeds/review.ts";

const f = (over: Partial<FeedFilters> = {}): FeedFilters => ({ ...NO_FEED_FILTERS, ...over });

test("the query: the text typed, then a term per choice; dismissed left out unless asked", () => {
  assert.equal(searchOf(f()), "is:new -is:dismissed");
  assert.equal(searchOf(f({ view: "all" })), "-is:dismissed");
  assert.equal(
    searchOf(f({ query: " greenland ", view: "shortlist", kind: "preprint", interest: "Ice sheets", source: "The Cryosphere" })),
    'greenland is:shortlisted kind:preprint prof:"Ice sheets" src:"The Cryosphere" -is:dismissed',
  );
  assert.equal(searchOf(f({ view: "all", query: "is:dismissed" })), "is:dismissed");
  assert.equal(facet("prof", 'say "hi"'), 'prof:"say hi"');
  assert.equal(facet("src", "EGUsphere"), "src:EGUsphere");
});

test("the segment asks for its kinds; a kind chosen moves the segment", () => {
  assert.deepEqual(requestOf(f()).kinds, ["paper", "preprint"]);
  assert.deepEqual(requestOf(f({ segment: "posts" })).kinds, ["news", "blog"]);
  assert.deepEqual(requestOf(f({ segment: "all" })).kinds, []);
  assert.deepEqual(chooseKind(f(), "news"), f({ kind: "news", segment: "posts" }));
  assert.deepEqual(chooseKind(f({ segment: "all" }), "news"), f({ kind: "news", segment: "all" }));
  assert.equal(chooseKind(f({ kind: "news", segment: "posts" }), "news").kind, null);
  assert.equal(chooseSegment(f({ kind: "news", segment: "posts" }), "papers").kind, null);
  assert.equal(chooseSegment(f({ kind: "news", segment: "posts" }), "all").kind, "news");
  assert.equal(chooseInterest(chooseInterest(f(), "AMOC"), "AMOC").interest, null);
});

test("chips, and their ×", () => {
  const all = f({ view: "unread", kind: "paper", interest: "AMOC", source: "GRL", query: "melt" });
  assert.deepEqual(
    feedChips(all).map((c) => c.text),
    ["Unread", "Papers", "AMOC", "GRL", '"melt"'],
  );
  assert.deepEqual(feedChips(f({ view: "all" })), []);
  const [view, kind, interest, source, search] = feedChips(all);
  assert.equal(withoutChip(all, view).view, "all");
  assert.equal(withoutChip(all, kind).kind, null);
  assert.equal(withoutChip(all, interest).interest, null);
  assert.equal(withoutChip(all, source).source, null);
  assert.equal(withoutChip(all, search).query, "");
});

test("restored: what is missing or wrong takes the default", () => {
  assert.deepEqual(restoredFeeds(null), NO_FEED_FILTERS);
  assert.deepEqual(restoredFeeds({ view: "nope" as never, sort: "newest", open: ["news", 3 as never] }), {
    ...NO_FEED_FILTERS,
    sort: "newest",
    open: ["news"],
  });
  assert.equal(restoredFeeds({ sort: "like" }).sort, "like");
  assert.equal(restoredFeeds({ sort: "oldest" as never }).sort, "score");
});

test("the sorts in turn, and a likeness as the list shows it", () => {
  assert.deepEqual([nextSort("score"), nextSort("like"), nextSort("newest")], ["like", "newest", "score"]);
  assert.deepEqual(requestOf(f({ sort: "like" })).sort, "like");
  assert.deepEqual([likeText(null), likeText(0), likeText(34.6)], ["–", "0", "35"]);
});

test("the counts of the views, and the sources in their groups", () => {
  assert.deepEqual(shelfCounts(null), { unread: 0, shortlist: 0, added: 0, all: 0 });
  assert.deepEqual(shelfCounts({ all: 9, is: { new: 4, shortlisted: 2 }, kinds: {}, interests: {}, sources: {} }), {
    unread: 4,
    shortlist: 2,
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
      sub("TC", "journal", { rss: "x" }),
      sub("GRL", "journal", { rss: "y" }),
      sub("Ice", "journal", { openalex: "z" }),
      sub("Eos", "news", { rss: "e" }),
    ],
    { TC: 3, GRL: 1, Ice: 5 },
  );
  assert.deepEqual(
    groups.map((g) => [g.group, g.count, g.sources.map((s) => s.title)]),
    [
      ["journal", 4, ["GRL", "TC"]],
      ["sweeps", 5, ["Ice"]],
      ["news", 0, ["Eos"]],
    ],
  );
});

test("the feed of the Subscribe sheet, and the same feed", () => {
  assert.deepEqual(feedOf("rss", " https://x.org/feed "), { rss: "https://x.org/feed" });
  assert.deepEqual(feedOf("crossref", "1994-0424"), { crossref: "1994-0424" });
  assert.equal(feedOf("openalex", "  "), null);
  assert.ok(sameFeed({ rss: "https://x.org/feed/" }, { rss: "http://X.org/feed" }));
  assert.ok(sameFeed({ crossref: "1994-0424" }, { crossref: "19940424" }));
  assert.ok(!sameFeed({ rss: "x" }, { crossref: "x" }));
});

const it = (id: string, triage: Triage) => ({ id, triage });

test("the keys toggle the triage of the selection", () => {
  assert.deepEqual(toggled([it("a", "new"), it("b", "shortlisted")], "shortlist"), { ids: ["a"], triage: "shortlisted" });
  assert.deepEqual(toggled([it("a", "shortlisted"), it("b", "shortlisted")], "shortlist"), { ids: ["a", "b"], triage: "seen" });
  assert.deepEqual(toggled([it("a", "seen")], "dismiss"), { ids: ["a"], triage: "dismissed" });
  assert.deepEqual(toggled([it("a", "dismissed")], "dismiss"), { ids: ["a"], triage: "seen" });
  // m: the new ones seen; with none new, the seen ones new; the others keep theirs
  assert.deepEqual(toggled([it("a", "new"), it("b", "seen"), it("c", "added")], "read"), { ids: ["a"], triage: "seen" });
  assert.deepEqual(toggled([it("b", "seen"), it("c", "shortlisted")], "read"), { ids: ["b"], triage: "new" });
  assert.equal(toggled([it("c", "added")], "read"), null);
  assert.equal(toggled([], "shortlist"), null);
});

test("mark all above, ranges, the next after some go", () => {
  const rows = [it("a", "new"), it("b", "seen"), it("c", "new"), it("d", "new")];
  assert.deepEqual(newAbove(rows, "d"), ["a", "c"]);
  assert.deepEqual(newAbove(rows, "a"), []);
  assert.deepEqual(newAbove(rows, null), ["a", "c", "d"]);
  assert.deepEqual(rangeOf(["a", "b", "c", "d"], "c", "a"), ["a", "b", "c"]);
  assert.deepEqual(rangeOf(["a", "b"], "z", "b"), ["b"]);
  assert.equal(nextAfter(["a", "b", "c", "d"], new Set(["b", "c"])), "d");
  assert.equal(nextAfter(["a", "b", "c"], new Set(["c"])), "b");
  assert.equal(nextAfter(["a"], new Set(["a"])), null);
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
  interests: [],
  like: null,
  triage,
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
  assert.equal(arxivDoi("2410.01234v2"), "10.48550/arXiv.2410.01234");
  assert.equal(arxivDoi("arXiv:hep-th/9901001"), "10.48550/arXiv.hep-th/9901001");
  assert.equal(arxivDoi("10.1/x"), null);
  assert.equal(addField(" 10.1/x "), "10.1/x");
});

test("ages, staleness and the status line", () => {
  const now = Date.parse("2026-10-31T12:00:00Z");
  const at = (minutes: number) => new Date(now - minutes * 60000).toISOString();
  assert.equal(ago(at(0), now), "now");
  assert.equal(ago(at(8), now), "8 min");
  assert.equal(ago(at(150), now), "2 h");
  assert.equal(ago(at(3 * 24 * 60), now), "3 d");
  assert.equal(ago(null, now), "");
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
      { interest: "AMOC", score: 4, terms: [], reached: true },
      { interest: "Ice", score: 1, terms: [], reached: false },
    ],
  };
  assert.deepEqual(
    badges(r).map((b) => b.text),
    ["in library", "has preprint", "2 news", "AMOC"],
  );
  assert.deepEqual(badges({ ...row("p"), kind: "news", links: 1 }).map((b) => b.text), ["→ paper"]);
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

test("the review steps through the shortlist", () => {
  let r = startReview(["a", "b", "c"]);
  assert.equal(reviewed(r), "a");
  assert.deepEqual(progressOf(r), { text: "1 of 3", fraction: 0 });
  r = decide(r, "added");
  assert.equal(reviewed(r), "b");
  r = stepReview(r, -5);
  assert.equal(reviewed(r), "a");
  r = stepReview(r, 1);
  r = decide(r, "later");
  r = decide(r, "dismissed");
  assert.equal(reviewed(r), null);
  assert.equal(progressOf(r).text, "3 of 3, done");
  assert.equal(decide(r, "added"), r);
  assert.equal(summaryOf(r), "1 added, 1 dismissed, 1 for later");
  assert.equal(summaryOf(stepReview(startReview(["a", "b"]), 9)), "0 added, 0 dismissed, 0 for later, 2 passed");
  assert.equal(progressOf(startReview([])).text, "nothing shortlisted");
});
