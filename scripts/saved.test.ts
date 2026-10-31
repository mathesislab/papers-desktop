// Saved searches: the list of the sidebars, and the queries of the Feeds tab taken apart and put together again.
// node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  NO_FEED_FILTERS,
  applied,
  decompose,
  isShown,
  queryOf,
  sameQuery,
  type FeedFilters,
} from "../src/lib/feeds/filters.ts";
import { NO_FILTERS, sameFilters } from "../src/lib/library/filters.ts";
import { added, changed, refusal, removed, renamed, sameName } from "../src/lib/saved.ts";
import { FIELDS, queryTerms } from "../src/lib/query.ts";

const f = (over: Partial<FeedFilters> = {}): FeedFilters => ({ ...NO_FEED_FILTERS, ...over });

test("the list: a name once in any case, added at the end, renamed, changed, removed in place", () => {
  const list = [
    { name: "AMOC", n: 1 },
    { name: "Ice", n: 2 },
  ];
  assert.ok(sameName(" amoc ", "AMOC"));
  assert.equal(refusal(list, "  "), "A name is needed");
  assert.equal(refusal(list, "ice"), "A saved search has that name");
  assert.equal(refusal(list, "Firn"), null);
  // renaming: its own name, in another case, is no refusal
  assert.equal(refusal(list, "ICE", "Ice"), null);
  assert.equal(refusal(list, "amoc", "Ice"), "A saved search has that name");
  assert.deepEqual(added(list, { name: " Firn ", n: 3 }).map((s) => s.name), ["AMOC", "Ice", "Firn"]);
  assert.deepEqual(renamed(list, "amoc", " Overturning "), [{ name: "Overturning", n: 1 }, list[1]]);
  assert.deepEqual(changed(list, "Ice", (s) => ({ ...s, n: 9 })), [list[0], { name: "Ice", n: 9 }]);
  assert.deepEqual(removed(list, "ice"), [list[0]]);
});

/** The terms of a query as a set: two queries the same in any order. */
const termsOf = (q: string) => queryTerms(q, [...FIELDS, "kind", "is", "src", "prof", "score"]).map((t) => t.text).sort();

test("a query of the feeds taken apart into the choices, and put together again", () => {
  assert.deepEqual(decompose('is:shortlisted greenland kind:preprint prof:"Ice sheets" src:"The Cryosphere",GRL'), {
    view: "shortlist",
    kinds: ["preprint"],
    interests: ["Ice sheets"],
    sources: ["The Cryosphere", "GRL"],
    segment: "papers",
    query: "greenland",
  });
  // the kinds of a segment: that segment, no kind chosen; no kind: All
  assert.deepEqual(decompose("kind:news,blog is:unread"), { ...decompose(""), view: "unread", segment: "posts" });
  assert.deepEqual(decompose("melt"), { view: "all", kinds: [], interests: [], sources: [], segment: "all", query: "melt" });
  assert.deepEqual(decompose("kind:paper,news").kinds, ["paper", "news"]);
  assert.equal(decompose("kind:paper,news").segment, "all");
  // what the sidebar does not choose stays text: an excluding term, a second of a group, another `is:`, a facet value no kind
  assert.equal(
    decompose('-src:Eos is:new is:shortlisted kind:podcast src:a src:b "is:new" score:3-').query,
    '-src:Eos is:shortlisted kind:podcast src:b "is:new" score:3-',
  );

  const queries = [
    "",
    "is:new kind:paper,preprint",
    'greenland is:shortlisted kind:preprint prof:"Ice sheets" src:"The Cryosphere",GRL',
    'src:tc,"Nature Geosci" t:"ice sheet" -kind:news a:moreau,ito y:2020-',
    "is:dismissed kind:news,blog prof:AMOC,Firn melt -is:seen",
    "kind:blog,paper score:3- is:added",
    'is:new is:seen src:"a, b" src:c',
  ];
  for (const q of queries) {
    const back = queryOf({ ...NO_FEED_FILTERS, ...decompose(q) });
    assert.deepEqual(termsOf(back), termsOf(q), `${q} → ${back}`);
    assert.ok(sameQuery(q, back));
  }
  // the choices of the tab come back as they were (the kinds of a segment as the segment)
  const choices = [
    f(),
    f({ view: "all", segment: "all", query: "melt y:2020-" }),
    f({ view: "added", kinds: ["news"], segment: "posts", interests: ["A b", "C"], sources: ["Eos"] }),
    f({ kinds: ["paper", "news"], segment: "all" }),
  ];
  for (const c of choices) assert.deepEqual({ ...c, ...decompose(queryOf(c)) }, c);
  assert.ok(sameQuery("prof:A is:new melt", "is:new melt prof:A"));
  assert.ok(!sameQuery("prof:A", "prof:B"));
});

test("a saved search applied, and the one the tab shows", () => {
  const search = { name: "x", query: "is:shortlisted kind:news,blog prof:AMOC", sort: "newest" as const };
  const shown = applied(f({ open: ["blog"], query: "old", sources: ["TC"] }), search);
  assert.deepEqual(shown, f({ view: "shortlist", segment: "posts", interests: ["AMOC"], sort: "newest", open: ["blog"] }));
  assert.ok(isShown(shown, search));
  assert.ok(!isShown({ ...shown, sort: "score" }, search));
  assert.ok(!isShown({ ...shown, query: "melt" }, search));
});

test("the filters of the library the same, in any order", () => {
  const a = { ...NO_FILTERS, bibs: ["/a", "/b"], tags: [["amoc", "ice"], ["x"]], query: " melt " };
  assert.ok(sameFilters(a, { ...a, bibs: ["/b", "/a"], tags: [["x"], ["ICE", "amoc"]], query: "melt" }));
  assert.ok(!sameFilters(a, { ...a, bibs: ["/a"] }));
  assert.ok(!sameFilters(a, { ...a, tags: [["amoc"], ["ice"], ["x"]] }));
  assert.ok(!sameFilters(a, { ...a, view: "nopdf" }));
  assert.ok(sameFilters(NO_FILTERS, { ...NO_FILTERS }));
});
