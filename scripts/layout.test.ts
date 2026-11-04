// The layout of the main window: the defaults, a layout of an older version read without losing anything, the
// layout saved read back as it was. node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { NO_FEED_FILTERS } from "../src/lib/feeds/filters.ts";
import { LAYOUT_VERSION, cleared, restore, stale } from "../src/lib/layout.ts";
import { NO_FILTERS } from "../src/lib/library/filters.ts";
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
  assert.deepEqual(layout.search, { ...BEFORE.search, authors: "", columns: BEFORE.searchColumns, look: "table" });
  for (const key of ["view", "feedColumns", "feedsShown", "searchColumns"]) assert.ok(!(key in layout), key);
});

test("the Feeds tab's columns of an older version still, widths by column: in their first order", () => {
  const layout = restore({ feedColumns: { score: 90, journal: 120 } } as never);
  assert.deepEqual(layout.feeds.columns.slice(0, 4), [
    { key: "status", width: 48 },
    { key: "score", width: 90 },
    { key: "date", width: 58 },
    { key: "journal", width: 120 },
  ]);
});

test("the Similar column of before (now a part of the score): the Score column in its place, unless shown too", () => {
  const keys = (columns: { key: string }[]) => columns.map((c) => c.key);
  const shown = (...keys: string[]) => keys.map((key) => ({ key, width: key === "similar" ? 72 : 100 }));
  const feeds = shown("date", "journal", "title", "authors", "similar", "score", "badges");
  const search = shown("cited", "similar", "title", "authors", "journal", "year", "badges");
  const layout = restore({ feeds: { filters: {}, columns: feeds }, search: { columns: search } } as never);
  // the Badges column of before too: the Status column, at its own width
  assert.deepEqual(keys(layout.feeds.columns), ["date", "journal", "title", "authors", "score", "status"]);
  assert.deepEqual(keys(layout.search.columns), ["cited", "score", "title", "authors", "journal", "year", "status"]);
  assert.deepEqual(layout.feeds.columns.at(-1), { key: "status", width: 48 });
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

test("the Papers tab: a PDF in front of the library of before, the paper in front one of those open", () => {
  const before = restore({ tab: "library", open: ["a", "b"], active: "a" } as never);
  assert.equal(before.tab, "papers");
  assert.equal(before.active, "a");
  assert.equal(restore({ tab: "library", open: ["a"], active: null } as never).tab, "library");
  assert.equal(restore({ tab: "papers", open: [], active: null } as never).tab, "library", "no paper open");
  const gone = restore({ tab: "papers", open: ["a", "b"], active: "c" } as never);
  assert.deepEqual([gone.tab, gone.active], ["papers", "b"], "the last open");
  assert.equal(restore(null).paperDetails, false);
});

test("a tab not used for a while: its search cleared when it comes back", () => {
  const hour = 60 * 60_000;
  assert.equal(stale({ library: 0 }, "library", hour + 1, 60), true);
  assert.equal(stale({ library: 0 }, "library", hour, 60), false);
  assert.equal(stale({ library: 0 }, "library", 10 * hour, 0), false, "never");
  assert.equal(stale({}, "feeds", 10 * hour, 60), false, "not used yet");
  const layout = restore({
    library: { bibs: ["a.bib"], view: "recent", tags: ["ice"], query: "melt" },
    feeds: { filters: { view: "starred", kinds: ["news"], interests: [], sources: ["EOS"], sort: "newest", query: "amoc", open: ["news"] } },
    search: { query: "firn", authors: "Smith", year: "2020", recent: [{ query: "firn", authors: "", year: "", order: "score" }] },
    used: { library: 5, feeds: "x", other: 3 },
  } as never);
  assert.deepEqual(layout.used, { library: 5 }, "the times of the tabs only");
  assert.deepEqual(cleared(layout, "library").library, NO_FILTERS);
  assert.deepEqual(cleared(layout, "feeds").feeds.filters, { ...NO_FEED_FILTERS, sort: "newest", open: ["news"] });
  const search = cleared(layout, "search").search;
  assert.deepEqual([search.query, search.authors, search.year, search.results], ["", "", "", null]);
  assert.equal(search.recent.length, 1, "the recent searches kept");
});
