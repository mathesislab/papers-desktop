// The layout of the main window, kept between sessions (window.json, in the
// data folder of the app, through the commands `layout` and `save_layout`).

import { FEED_ORDER, FEED_SPECS, SEARCH_ORDER, SEARCH_SPECS, type FeedColumn } from "$lib/feeds/FeedTable.svelte";
import { NO_FEED_FILTERS, restoredFeeds, type FeedFilters } from "$lib/feeds/filters";
import { NO_FILTERS, restored, type Filters } from "$lib/library/filters";
import { COLUMNS, DEFAULT_COLUMNS, DEFAULT_ORDER, type Column, type Sort } from "$lib/library/Table.svelte";
import { columnsOf, restoredColumns, type Shown } from "$lib/list/columns";
import { NO_SEARCH, restoredSearch, type SearchState } from "$lib/search/query";

/** What the window keeps between sessions: the tab in front (Library,
 *  Feeds or Search), the widths of the side panes, whether the details show,
 *  the columns and order of the table, the library tab (its library, view,
 *  tags and search), the papers open and the page of each, the parent tags
 *  unfolded in the sidebar, the Feeds tab's choices and the widths of its
 *  columns, the Search tab's fields, recent searches and search shown, and
 *  the widths of its columns. */
export type Layout = {
  /** the tab of the window in front: the library (with the papers open), the feeds, or the search of OpenAlex */
  view: "library" | "feeds" | "search";
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
  /** the Feeds tab: the choices of its sidebar, the segment, the sort, the search, the groups unfolded */
  feeds: FeedFilters;
  feedColumns: Shown<FeedColumn>[];
  /** when the Feeds tab's list was last brought up to date (ISO): what was fetched since waits behind *N new* */
  feedsShown: string | null;
  /** the Search tab: its fields, its recent searches, the search shown */
  search: SearchState;
  searchColumns: Shown<FeedColumn>[];
};

const DEFAULTS: Layout = {
  view: "library",
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
  feeds: NO_FEED_FILTERS,
  feedColumns: columnsOf(FEED_ORDER, FEED_SPECS),
  feedsShown: null,
  search: NO_SEARCH,
  searchColumns: columnsOf(SEARCH_ORDER, SEARCH_SPECS),
};

/** The layout saved (none: the defaults): what a file of an older version
 *  does not have takes its default, and the filters of the library tab and of
 *  the Feeds tab, and the Search tab's, are checked (`restored`,
 *  `restoredFeeds`, `restoredSearch`), as are the columns of the three tables
 *  (`restoredColumns`: those of the Feeds and Search tabs, once widths by
 *  column, become a list in their first order, at the widths saved). */
export function restore(saved: Partial<Layout> | null): Layout {
  return {
    ...DEFAULTS,
    ...saved,
    columns: restoredColumns(saved?.columns, COLUMNS, DEFAULT_ORDER),
    library: restored(saved?.library),
    feeds: restoredFeeds(saved?.feeds),
    feedColumns: restoredColumns(saved?.feedColumns, FEED_SPECS, FEED_ORDER),
    search: restoredSearch(saved?.search),
    searchColumns: restoredColumns(saved?.searchColumns, SEARCH_SPECS, SEARCH_ORDER),
  };
}
