// The layout of the main window, kept between sessions (window.json, in the data folder of the app, through the
// commands `layout` and `save_layout`), and that of an older version read. Pure, tested by scripts/layout.test.ts
// (npm test).

import { FEED_ORDER, FEED_SPECS, SEARCH_ORDER, SEARCH_SPECS, similarAsScore, type FeedColumn } from "./feeds/columns.ts";
import { NO_FEED_FILTERS, restoredFeeds, type FeedFilters } from "./feeds/filters.ts";
import { COLUMNS, DEFAULT_COLUMNS, DEFAULT_ORDER, type Column, type Sort } from "./library/columns.ts";
import { NO_FILTERS, restored, type Filters } from "./library/filters.ts";
import { columnsOf, restoredColumns, restoredLook, withColumn, type Look, type Shown } from "./list/columns.ts";
import { NO_SEARCH, restoredSearch, type SearchState } from "./search/state.ts";

/** A tab of the window: the library (with the papers open), the feeds, or the search of OpenAlex. */
export type Tab = "library" | "feeds" | "search";

/** The Feeds tab: the choices of its sidebar, the segment, the sort, the search, the groups unfolded; its list's
 *  columns, and whether it shows them or is simple; when its list was last brought up to date (ISO): what was fetched
 *  since waits behind *N new*. */
export type FeedsLayout = { filters: FeedFilters; columns: Shown<FeedColumn>[]; look: Look; shown: string | null };

/** The Search tab: its fields, its recent searches, the search shown; its list's columns, and whether it shows them
 *  or is simple. */
export type SearchLayout = SearchState & { columns: Shown<FeedColumn>[]; look: Look };

/** What the window keeps between sessions: the tab in front, the widths of the side panes, whether the details
 *  show, the columns, order and look of the library's list, the library tab (its library, view, tags and search), the
 *  papers open and the page of each, the parent tags unfolded in the sidebar, the Feeds and the Search tab's own. */
export type Layout = {
  /** what the layout knows: `LAYOUT_VERSION` once read */
  version: number;
  tab: Tab;
  sidebar: number;
  details: number;
  showSidebar: boolean;
  showDetails: boolean;
  columns: Shown<Column>[];
  sort: Sort;
  /** the library's list: a table of its columns, or simple */
  look: Look;
  /** the filters of the library tab: the current set it shows */
  library: Filters;
  open: string[];
  /** the paper in front; none: the library */
  active: string | null;
  pages: Record<string, number>;
  /** the parent tags unfolded in the sidebar, folded as `foldTag` does */
  openTags: string[];
  feeds: FeedsLayout;
  search: SearchLayout;
};

/** What an older version kept apart, beside the keys of `Layout`: `view` for `tab`, the Feeds tab's filters in
 *  `feeds` and its columns and list shown in `feedColumns` and `feedsShown`, the Search tab's columns in
 *  `searchColumns`. */
type Before = {
  view?: unknown;
  feedColumns?: unknown;
  feedsShown?: unknown;
  searchColumns?: unknown;
};

const TABS: Tab[] = ["library", "feeds", "search"];

/** The version of the layout: 2 since the library's star column, which a layout of before shows once (none: 1). */
export const LAYOUT_VERSION = 2;

const DEFAULTS: Layout = {
  version: LAYOUT_VERSION,
  tab: "library",
  sidebar: 230,
  details: 340,
  showSidebar: true,
  showDetails: true,
  columns: DEFAULT_COLUMNS,
  sort: null,
  look: "table",
  library: NO_FILTERS,
  open: [],
  active: null,
  pages: {},
  openTags: [],
  feeds: { filters: NO_FEED_FILTERS, columns: columnsOf(FEED_ORDER, FEED_SPECS), look: "simple", shown: null },
  search: { ...NO_SEARCH, columns: columnsOf(SEARCH_ORDER, SEARCH_SPECS), look: "table" },
};

/** The layout saved (none: the defaults): what a file of an older version does not have takes its default (one of
 *  before version 2 shows the library's star column; the Feeds tab's list, simple; the library's and the Search
 *  tab's, a table), and what it kept apart (`Before`) comes in its place; the filters of the library tab and of the
 *  Feeds tab, and the Search tab's, are checked (`restored`, `restoredFeeds`, `restoredSearch`), as are the columns
 *  of the three tables (`restoredColumns`: those of the Feeds and Search tabs, once widths by column, become a list
 *  in their first order, at the widths saved; their Similar column of before is the Score column, `similarAsScore`)
 *  and the looks of the three lists (`restoredLook`). */
export function restore(saved: (Partial<Layout> & Before) | null): Layout {
  const { view, feedColumns, feedsShown, searchColumns, ...kept } = saved ?? {};
  const tab = [kept.tab, view].find((t): t is Tab => TABS.includes(t as Tab)) ?? DEFAULTS.tab;
  // the Feeds tab's own, nested, or its filters alone (an older version)
  const feeds: Partial<Record<keyof FeedsLayout, unknown>> =
    kept.feeds && "filters" in kept.feeds ? kept.feeds : { filters: kept.feeds, columns: feedColumns, shown: feedsShown };
  const search = kept.search;
  const columns = restoredColumns(kept.columns, COLUMNS, DEFAULT_ORDER);
  // the star column, new in version 2: shown in a layout of before, where it was in no menu; hidden since, it stays so
  const before = typeof kept.version !== "number" || kept.version < 2;
  return {
    ...DEFAULTS,
    ...kept,
    version: LAYOUT_VERSION,
    tab,
    columns: before ? withColumn(columns, COLUMNS, DEFAULT_ORDER, "star") : columns,
    look: restoredLook(kept.look, "table"),
    library: restored(kept.library),
    feeds: {
      filters: restoredFeeds(feeds.filters as Partial<FeedFilters> | undefined),
      columns: restoredColumns(similarAsScore(feeds.columns), FEED_SPECS, FEED_ORDER),
      look: restoredLook(feeds.look, "simple"),
      shown: typeof feeds.shown === "string" ? feeds.shown : null,
    },
    search: {
      ...restoredSearch(search),
      columns: restoredColumns(similarAsScore(search?.columns ?? searchColumns), SEARCH_SPECS, SEARCH_ORDER),
      look: restoredLook(search?.look, "table"),
    },
  };
}
