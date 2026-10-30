// The layout of the main window, kept between sessions (window.json, in the
// data folder of the app, through the commands `layout` and `save_layout`).

import { NO_FILTERS, restored, type Filters } from "$lib/library/filters";
import { DEFAULT_COLUMNS, type Shown, type Sort } from "$lib/library/Table.svelte";

/** What the window keeps between sessions: the tab in front (Library or
 *  Feeds), the widths of the side panes, whether the details show, the
 *  columns and order of the table, the library tab (its library, view, tags
 *  and search), the papers open and the page of each, the parent tags
 *  unfolded in the sidebar. */
export type Layout = {
  /** the tab of the window in front: the library (with the papers open), or the feeds */
  view: "library" | "feeds";
  sidebar: number;
  details: number;
  showSidebar: boolean;
  showDetails: boolean;
  columns: Shown[];
  sort: Sort;
  /** the filters of the library tab: the current set it shows */
  library: Filters;
  open: string[];
  /** the paper in front; none: the library */
  active: string | null;
  pages: Record<string, number>;
  /** the parent tags unfolded in the sidebar, folded as `foldTag` does */
  openTags: string[];
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
};

/** The layout saved (none: the defaults): what a file of an older version
 *  does not have takes its default, and the filters of the library tab are
 *  checked (`restored`). */
export function restore(saved: Partial<Layout> | null): Layout {
  return { ...DEFAULTS, ...saved, library: restored(saved?.library) };
}
