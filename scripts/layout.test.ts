// The layout of the main window: the defaults, a layout of an older version read without losing anything, the
// layout saved read back as it was. node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { LAYOUT_VERSION, restore } from "../src/lib/layout.ts";
import { COLUMNS } from "../src/lib/library/columns.ts";

test("no layout: the defaults, the Library tab in front", () => {
  const layout = restore(null);
  assert.equal(layout.tab, "library");
  assert.equal(layout.feeds.shown, null);
  assert.equal(layout.feeds.filters.view, "unread");
  assert.equal(layout.feeds.look, "calm");
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
  assert.equal(layout.feeds.look, "calm");
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

test("the library's star column shown once in a layout of before version 2, then hidden if hidden", () => {
  const before = { columns: [{ key: "authors", width: 90 }, { key: "title", width: 300 }] };
  const shown = restore(before as never);
  assert.equal(shown.version, LAYOUT_VERSION);
  assert.deepEqual(shown.columns, [{ key: "star", width: COLUMNS.star.width }, ...before.columns]);
  // at its place in the order at first: before the first column shown that comes after it there
  assert.deepEqual(
    restore({ columns: [{ key: "doi", width: 190 }, { key: "year", width: 56 }] } as never).columns.map((c) => c.key),
    ["doi", "star", "year"],
  );
  // shown already, or a layout of version 2: as saved
  const moved = [{ key: "title", width: 300 }, { key: "star", width: 40 }];
  assert.deepEqual(restore({ columns: moved } as never).columns, moved);
  const hidden = { ...JSON.parse(JSON.stringify(shown)), columns: before.columns };
  assert.deepEqual(restore(hidden).columns, before.columns);
});

test("the Feeds tab's list calm unless a table was chosen", () => {
  const nested = { filters: {}, columns: [], shown: null };
  assert.equal(restore({ feeds: nested } as never).feeds.look, "calm");
  assert.equal(restore({ feeds: { ...nested, look: "table" } } as never).feeds.look, "table");
  assert.equal(restore({ feeds: { ...nested, look: "cards" } } as never).feeds.look, "calm");
});

test("the Starred view of the Feeds tab by its name of before", () => {
  assert.equal(restore({ feeds: { view: "shortlist" } } as never).feeds.filters.view, "starred");
});

test("the layout saved read back as it was; a tab unknown is the library", () => {
  const layout = restore(BEFORE as never);
  assert.deepEqual(restore(JSON.parse(JSON.stringify(layout))), layout);
  assert.equal(restore({ tab: "elsewhere" } as never).tab, "library");
});
