// The columns of the library's table (Table): their names and widths at first, those shown at first, the sort.

import { columnsOf, type Specs } from "../list/columns.ts";

export type Column = "authors" | "year" | "title" | "container" | "tags" | "key" | "kind" | "doi" | "bib" | "added";
/** a column and its direction; none: the order of the library (newest first) */
export type Sort = { column: Column; descending: boolean } | null;

/** the columns, in the order of the menu, with their width at first */
export const COLUMNS: Specs<Column> = {
  authors: { name: "Authors", width: 150 },
  year: { name: "Year", width: 56 },
  title: { name: "Title", width: 320 },
  container: { name: "Journal", width: 170 },
  tags: { name: "Tags", width: 120 },
  key: { name: "Key", width: 140 },
  kind: { name: "Type", width: 90 },
  doi: { name: "DOI", width: 190 },
  bib: { name: "Library", width: 120 },
  added: { name: "Added", width: 90 },
};

/** the columns shown at first */
export const DEFAULT_ORDER: Column[] = ["authors", "year", "title", "container", "tags"];
export const DEFAULT_COLUMNS = columnsOf(DEFAULT_ORDER, COLUMNS);
