// The Search tab: the query of the fields, the years, the recent searches, the layout kept, the works around a work.
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
  moreText,
  recentText,
  restoredSearch,
  totalText,
  withRecent,
  yearError,
  yearValue,
  type Asked,
} from "../src/lib/search/query.ts";

const a = (query: string, year = "", order: Asked["order"] = "relevance"): Asked => ({ query, year, order });

test("the Year field: a year, a span, open at either end, spaces left out", () => {
  assert.equal(yearValue(""), "");
  assert.equal(yearValue("  "), "");
  assert.equal(yearValue("2005"), "2005");
  assert.equal(yearValue("2000-2010"), "2000-2010");
  assert.equal(yearValue(" 2000 - 2010 "), "2000-2010");
  assert.equal(yearValue("2000-"), "2000-");
  assert.equal(yearValue("-2010"), "-2010");
  assert.equal(yearValue("2010-2010"), "2010-2010");
});

test("the Year field: what is not years", () => {
  for (const wrong of ["-", "20", "200", "20055", "2010-2000", "2000-10", "abc", "2000–2010", "2000,2010", "y:2000"]) {
    assert.equal(yearValue(wrong), null, wrong);
    assert.match(yearError(wrong) ?? "", /not 2005, 2000-2010, 2000- or -2010/, wrong);
  }
  assert.equal(yearError(""), null);
  assert.equal(yearError("2000-2010"), null);
});

test("the query asked: the field's text, then the years as y:", () => {
  assert.equal(composed("greenland ice sheet", "2000-2010"), "greenland ice sheet y:2000-2010");
  assert.equal(composed("  greenland  ", ""), "greenland");
  assert.equal(composed("", "2005"), "y:2005");
  assert.equal(composed('t:"surface mass balance"', " -2010 "), 't:"surface mass balance" y:-2010');
  assert.equal(composed("", ""), "");
  assert.equal(composed("firn", "20-30"), null);
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
  let list: Asked[] = [];
  for (let i = 0; i < 30; i++) list = withRecent(list, a(`q${i}`));
  assert.equal(list.length, RECENT_MAX);
  assert.equal(list[0].query, "q29");
  assert.equal(list.at(-1)?.query, "q10");
  assert.equal(withRecent(list, a("x"), 5).length, 5);
});

test("a recent search as the sidebar says it", () => {
  assert.equal(recentText(a("greenland ice sheet", "2000-2010", "cited")), "greenland ice sheet · 2000-2010 · Most cited");
  assert.equal(recentText(a("firn")), "firn");
  assert.equal(recentText(a("", "2005", "newest")), "2005 · Newest");
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
  assert.deepEqual(restoredSearch(saved), {
    query: "firn",
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
  assert.equal(aroundQuery({ openalex: null, doi: "10.5194/tc-1-2" }, "citedby"), "citedby:10.5194/tc-1-2");
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
