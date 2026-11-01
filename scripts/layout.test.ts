// The layout of the main window: the defaults, a layout of an older version read without losing anything, the
// layout saved read back as it was. node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { restore } from "../src/lib/layout.ts";

test("no layout: the defaults, the Library tab in front", () => {
  const layout = restore(null);
  assert.equal(layout.tab, "library");
  assert.equal(layout.feeds.shown, null);
  assert.equal(layout.feeds.filters.view, "unread");
  assert.ok(layout.feeds.columns.some((c) => c.key === "score"));
  assert.ok(layout.search.columns.some((c) => c.key === "cited"));
});

/** a window.json of the version before the tabs' keys were nested */
const BEFORE = {
  view: "search",
  sidebar: 200,
  feeds: { view: "all", kinds: ["paper"], interests: [], sources: ["EOS"], sort: "newest", query: "melt", open: [] },
  feedColumns: [
    { key: "title", width: 300 },
    { key: "date", width: 70 },
  ],
  feedsShown: "2026-10-30T08:00:00.000Z",
  search: { query: "firn", year: "2020-", order: "cited", recent: [], results: null },
  searchColumns: [{ key: "cited", width: 80 }],
};

test("a layout of an older version: the tab in front, the Feeds and Search tabs' own nested, nothing lost", () => {
  const layout = restore(BEFORE as never);
  assert.equal(layout.tab, "search");
  assert.equal(layout.sidebar, 200);
  assert.deepEqual(layout.feeds.filters, BEFORE.feeds);
  assert.deepEqual(layout.feeds.columns, BEFORE.feedColumns);
  assert.equal(layout.feeds.shown, BEFORE.feedsShown);
  assert.deepEqual(layout.search, { ...BEFORE.search, columns: BEFORE.searchColumns });
  for (const key of ["view", "feedColumns", "feedsShown", "searchColumns"]) assert.ok(!(key in layout), key);
});

test("the Feeds tab's columns of an older version still, widths by column: in their first order", () => {
  const layout = restore({ feedColumns: { score: 90, title: 250 } } as never);
  assert.deepEqual(layout.feeds.columns.slice(0, 2), [
    { key: "score", width: 90 },
    { key: "similar", width: 70 },
  ]);
});

test("the Starred view of the Feeds tab by its name of before", () => {
  assert.equal(restore({ feeds: { view: "shortlist" } } as never).feeds.filters.view, "starred");
});

test("the layout saved read back as it was; a tab unknown is the library", () => {
  const layout = restore(BEFORE as never);
  assert.deepEqual(restore(JSON.parse(JSON.stringify(layout))), layout);
  assert.equal(restore({ tab: "elsewhere" } as never).tab, "library");
});
