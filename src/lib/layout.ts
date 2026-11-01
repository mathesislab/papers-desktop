// The layout of the main window, kept between sessions (window.json, in the data folder of the app, through the
// commands `layout` and `save_layout`), and that of an older version read. Pure, tested by scripts/layout.test.ts
// (npm test).

import { FEED_ORDER, FEED_SPECS, SEARCH_ORDER, SEARCH_SPECS, type FeedColumn } from "./feeds/columns.ts";
import { NO_FEED_FILTERS, restoredFeeds, type FeedFilters } from "./feeds/filters.ts";
import { COLUMNS, DEFAULT_COLUMNS, DEFAULT_ORDER, type Column, type Sort } from "./library/columns.ts";
import { NO_FILTERS, restored, type Filters } from "./library/filters.ts";
import { columnsOf, restoredColumns, type Shown } from "./list/columns.ts";
import { NO_SEARCH, restoredSearch, type SearchState } from "./search/state.ts";

/** A tab of the window: the library (with the papers open), the feeds, or the search of OpenAlex. */
export type Tab = "library" | "feeds" | "search";

/** The Feeds tab: the choices of its sidebar, the segment, the sort, the search, the groups unfolded; its list's
 *  columns; when its list was last brought up to date (ISO): what was fetched since waits behind *N new*. */
export type FeedsLayout = { filters: FeedFilters; columns: Shown<FeedColumn>[]; shown: string | null };

/** The Search tab: its fields, its recent searches, the search shown; its list's columns. */
export type SearchLayout = SearchState & { columns: Shown<FeedColumn>[] };

/** What the window keeps between sessions: the tab in front, the widths of the side panes, whether the details
 *  show, the columns and order of the library's table, the library tab (its library, view, tags and search), the
 *  papers open and the page of each, the parent tags unfolded in the sidebar, the Feeds and the Search tab's own. */
export type Layout = {
  tab: Tab;
  sidebar: number;
  details: number;
  showSidebar: boolean;
  showDetails: boolean;
  columns: Shown<Column>[];
  sort: Sort;
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

const DEFAULTS: Layout = {
  tab: "library",
  sidebar: 230,
  details: 340,
  showSidebar: true,
  showDetails: true,
  columns: DEFAULT_COLUMNS,
  sort: null,
  library: NO_FILTERS,
  open: [],
  active: null,
  pages: {},
  openTags: [],
  feeds: { filters: NO_FEED_FILTERS, columns: columnsOf(FEED_ORDER, FEED_SPECS), shown: null },
  search: { ...NO_SEARCH, columns: columnsOf(SEARCH_ORDER, SEARCH_SPECS) },
};

/** The layout saved (none: the defaults): what a file of an older version does not have takes its default, and
 *  what it kept apart (`Before`) comes in its place; the filters of the library tab and of the Feeds tab, and the
 *  Search tab's, are checked (`restored`, `restoredFeeds`, `restoredSearch`), as are the columns of the three tables
 *  (`restoredColumns`: those of the Feeds and Search tabs, once widths by column, become a list in their first
 *  order, at the widths saved). */
export function restore(saved: (Partial<Layout> & Before) | null): Layout {
  const { view, feedColumns, feedsShown, searchColumns, ...kept } = saved ?? {};
  const tab = [kept.tab, view].find((t): t is Tab => TABS.includes(t as Tab)) ?? DEFAULTS.tab;
  // the Feeds tab's own, nested, or its filters alone (an older version)
  const feeds: Partial<Record<keyof FeedsLayout, unknown>> =
    kept.feeds && "filters" in kept.feeds ? kept.feeds : { filters: kept.feeds, columns: feedColumns, shown: feedsShown };
  const search = kept.search;
  return {
    ...DEFAULTS,
    ...kept,
    tab,
    columns: restoredColumns(kept.columns, COLUMNS, DEFAULT_ORDER),
    library: restored(kept.library),
    feeds: {
      filters: restoredFeeds(feeds.filters as Partial<FeedFilters> | undefined),
      columns: restoredColumns(feeds.columns, FEED_SPECS, FEED_ORDER),
      shown: typeof feeds.shown === "string" ? feeds.shown : null,
    },
    search: {
      ...restoredSearch(search),
      columns: restoredColumns(search?.columns ?? searchColumns, SEARCH_SPECS, SEARCH_ORDER),
    },
  };
}
