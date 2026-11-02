// The Search tab: the query of the fields, the recent searches, the layout kept, the works around a work.
// node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { citedText, yearOf } from "../src/lib/feeds/items.ts";
import {
  NO_SEARCH,
  RECENT_MAX,
  appended,
  aroundQuery,
  composed,
  inOrder,
  namesOf,
  plainWords,
  moreText,
  recentText,
  restoredSearch,
  totalText,
  withRecent,
  type Typed,
} from "../src/lib/search/state.ts";

const a = (query: string, year = "", order: Typed["order"] = "score", authors = ""): Typed => ({ query, authors, year, order });

test("the query asked: the field's text, the names as a:, the years as y:, spaces left out", () => {
  const q = (query: string, year = "", authors = "") => composed({ query, authors, year });
  assert.equal(q("greenland ice sheet", "2000-2010"), "greenland ice sheet y:2000-2010");
  assert.equal(q("  greenland  "), "greenland");
  assert.equal(q("", "2005"), "y:2005");
  assert.equal(q('t:"surface mass balance"', " -2010 "), 't:"surface mass balance" y:-2010');
  assert.equal(q("", ""), "");
  assert.equal(q("firn", " 2000 - 2010 "), "firn y:2000-2010");
  assert.equal(q("firn", "2015, 2020"), "firn y:2015,2020");
  // what is not years is Rust's to refuse
  assert.equal(q("firn", "20-30"), "firn y:20-30");
  assert.equal(q("ice sheet", "2020", "smith, Ben Smith"), 'ice sheet a:smith a:"Ben Smith" y:2020');
});

test("the names of the Author(s) field: by commas, semicolons, & or and", () => {
  assert.deepEqual(namesOf("Smith, Fricker"), ["Smith", "Fricker"]);
  assert.deepEqual(namesOf("smith AND fricker & Paolo;  van den Broeke "), ["smith", "fricker", "Paolo", "van den Broeke"]);
  assert.deepEqual(namesOf("Alexander  Robinson"), ["Alexander Robinson"]);
  assert.deepEqual(namesOf(" , "), []);
});

test("the search in plain words: for the library and the services online", () => {
  assert.equal(plainWords({ query: " ice sheet ", authors: "Smith, Fricker", year: "2020" }), "ice sheet Smith Fricker 2020");
  assert.equal(plainWords({ query: "", authors: "", year: "" }), "");
});

test("the works in the order of the search, the library's among them", () => {
  const w = (id: string, score: number, date: string | null, cited: number | null) => ({ id, score, date, cited });
  const rows = [w("a", 10, "2019-05-01", 3), w("lib", 80, "2020-01-01", null), w("b", 40, null, 50)];
  const ids = (order: Typed["order"]) => inOrder(rows, order).map((r) => r.id);
  assert.deepEqual(ids("score"), ["lib", "b", "a"]);
  assert.deepEqual(ids("newest"), ["lib", "a", "b"]);
  assert.deepEqual(ids("cited"), ["b", "a", "lib"]);
  assert.deepEqual(ids("relevance"), ["a", "lib", "b"]);
});

test("the recent searches: the latest first, the same search once, blank ones not kept", () => {
  let list = withRecent([], a("firn"));
  list = withRecent(list, a("greenland", "2000-2010"));
  assert.deepEqual(list, [a("greenland", "2000-2010"), a("firn")]);
  // the same text and years, spaces aside: in its place first, with its order now
  list = withRecent(list, a("  firn ", "", "cited"));
  assert.deepEqual(list, [a("firn", "", "cited"), a("greenland", "2000-2010")]);
  // other years: another search
  list = withRecent(list, a("greenland", "2005"));
  assert.equal(list.length, 3);
  assert.equal(withRecent(list, a("  ", " ")), list);
  // the years alone are a search
  assert.deepEqual(withRecent([], a("", " 2005 ")), [a("", "2005")]);
});

test("the recent searches: at most 20", () => {
  let list: Typed[] = [];
  for (let i = 0; i < 30; i++) list = withRecent(list, a(`q${i}`));
  assert.equal(list.length, RECENT_MAX);
  assert.equal(list[0].query, "q29");
  assert.equal(list.at(-1)?.query, "q10");
  assert.equal(withRecent(list, a("x"), 5).length, 5);
});

test("a recent search as the sidebar says it", () => {
  assert.equal(recentText(a("greenland ice sheet", "2000-2010", "cited")), "greenland ice sheet · 2000-2010 · Most cited");
  assert.equal(recentText(a("firn")), "firn");
  assert.equal(recentText(a("", "2005", "relevance")), "2005 · OpenAlex relevance");
});

test("the layout restored: the defaults for what is missing or wrong", () => {
  assert.deepEqual(restoredSearch(null), NO_SEARCH);
  assert.deepEqual(restoredSearch(undefined), NO_SEARCH);
  assert.deepEqual(restoredSearch({}), NO_SEARCH);
  const saved = {
    query: "firn",
    year: "2000-",
    order: "newest",
    recent: [a("firn", "2000-", "newest"), { query: 3 }, null, { query: "melt", year: "", order: "best" }],
    results: { query: "firn y:2000-", order: "newest", ids: ["a", 4, "b"], total: 1234, next: "cursor" },
  } as unknown as Parameters<typeof restoredSearch>[0];
  // a layout of before the Author(s) field: none
  assert.deepEqual(restoredSearch(saved), {
    query: "firn",
    authors: "",
    year: "2000-",
    order: "newest",
    recent: [a("firn", "2000-", "newest"), a("melt")],
    results: { query: "firn y:2000-", order: "newest", ids: ["a", "b"], total: 1234, next: "cursor" },
  });
  const wrong = { query: 1, year: null, order: "best", recent: "x", results: { query: "q", ids: "x", total: 1 } };
  assert.deepEqual(restoredSearch(wrong as unknown as Parameters<typeof restoredSearch>[0]), NO_SEARCH);
  const long = { recent: Array.from({ length: 30 }, (_, i) => a(`q${i}`)) };
  assert.equal(restoredSearch(long).recent.length, RECENT_MAX);
  // the last page: no cursor
  const last = { results: { query: "q", order: "cited", ids: [], total: 0, next: null } } as unknown as Parameters<typeof restoredSearch>[0];
  assert.deepEqual(restoredSearch(last).results, { query: "q", order: "cited", ids: [], total: 0, next: null });
});

test("the works around a work: by its OpenAlex id, else its DOI; none without either", () => {
  assert.equal(aroundQuery({ openalex: "W2741809807", doi: "10.5194/tc-1-2" }, "cites"), "cites:W2741809807");
  // a DOI quoted, as any value of a filter with a slash
  assert.equal(aroundQuery({ openalex: null, doi: "10.5194/tc-1-2" }, "citedby"), 'citedby:"10.5194/tc-1-2"');
  assert.equal(aroundQuery({ openalex: null, doi: "10.1000/a,b" }, "cites"), 'cites:"10.1000/a,b"');
  assert.equal(aroundQuery({ openalex: null, doi: null }, "cites"), null);
});

test("the counts said", () => {
  assert.equal(totalText(1234), "1,234 works");
  assert.equal(totalText(1), "1 work");
  assert.equal(totalText(0), "0 works");
  assert.equal(moreText(25, 1234), "Show 25 more");
  assert.equal(moreText(50, 60), "Show 10 more");
  assert.equal(citedText(null), "–");
  assert.equal(citedText(12345), "12,345");
  assert.equal(yearOf("2007-03-01T00:00:00Z"), "2007");
  assert.equal(yearOf(null), "");
});

test("a next page after the rows shown, those there already left out", () => {
  const r = (id: string) => ({ id });
  assert.deepEqual(appended([r("a"), r("b")], [r("b"), r("c")]), [r("a"), r("b"), r("c")]);
  assert.deepEqual(appended([], [r("a")]), [r("a")]);
});
