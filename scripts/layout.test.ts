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
  assert.equal(layout.feeds.look, "simple");
  assert.ok(layout.feeds.columns.some((c) => c.key === "score"));
  assert.ok(layout.search.columns.some((c) => c.key === "cited"));
  assert.equal(layout.search.look, "table");
  assert.equal(layout.look, "table");
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
  assert.equal(layout.feeds.look, "simple");
  assert.deepEqual(layout.search, { ...BEFORE.search, columns: BEFORE.searchColumns, look: "table" });
  for (const key of ["view", "feedColumns", "feedsShown", "searchColumns"]) assert.ok(!(key in layout), key);
});

test("the Feeds tab's columns of an older version still, widths by column: in their first order", () => {
  const layout = restore({ feedColumns: { score: 90, journal: 120 } } as never);
  assert.deepEqual(layout.feeds.columns.slice(0, 2), [
    { key: "date", width: 58 },
    { key: "journal", width: 120 },
  ]);
  assert.deepEqual(layout.feeds.columns[4], { key: "score", width: 90 });
});

test("the Similar column of before (now a part of the score): the Score column in its place, unless shown too", () => {
  const keys = (columns: { key: string }[]) => columns.map((c) => c.key);
  const shown = (...keys: string[]) => keys.map((key) => ({ key, width: key === "similar" ? 72 : 100 }));
  const feeds = shown("date", "journal", "title", "authors", "similar", "score", "badges");
  const search = shown("cited", "similar", "title", "authors", "journal", "year", "badges");
  const layout = restore({ feeds: { filters: {}, columns: feeds }, search: { columns: search } } as never);
  assert.deepEqual(keys(layout.feeds.columns), ["date", "journal", "title", "authors", "score", "badges"]);
  assert.deepEqual(keys(layout.search.columns), ["cited", "score", "title", "authors", "journal", "year", "badges"]);
  assert.deepEqual(layout.search.columns[1], { key: "score", width: 72 }, "at the width of the Similar column");
  const older = restore({ feedColumns: shown("title", "similar") } as never);
  assert.deepEqual(keys(older.feeds.columns), ["title", "score"]);
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

test("the Feeds tab's list simple unless a table was chosen", () => {
  const nested = { filters: {}, columns: [], shown: null };
  assert.equal(restore({ feeds: nested } as never).feeds.look, "simple");
  assert.equal(restore({ feeds: { ...nested, look: "table" } } as never).feeds.look, "table");
  assert.equal(restore({ feeds: { ...nested, look: "cards" } } as never).feeds.look, "simple");
  // its name of before
  assert.equal(restore({ feeds: { ...nested, look: "calm" } } as never).feeds.look, "simple");
});

test("the library's list a table unless simple was chosen", () => {
  assert.equal(restore({ look: "simple" } as never).look, "simple");
  assert.equal(restore({ look: "cards" } as never).look, "table");
});

test("the Search tab's list a table unless simple was chosen", () => {
  assert.equal(restore({ search: { columns: [] } } as never).search.look, "table");
  assert.equal(restore({ search: { columns: [], look: "simple" } } as never).search.look, "simple");
  assert.equal(restore({ search: { columns: [], look: "cards" } } as never).search.look, "table");
});

test("the Starred view of the Feeds tab by its name of before", () => {
  assert.equal(restore({ feeds: { view: "shortlist" } } as never).feeds.filters.view, "starred");
});

test("the layout saved read back as it was; a tab unknown is the library", () => {
  const layout = restore(BEFORE as never);
  assert.deepEqual(restore(JSON.parse(JSON.stringify(layout))), layout);
  assert.equal(restore({ tab: "elsewhere" } as never).tab, "library");
});
