// Mirrors papers_core::Item and the Status of src-tauri/src/lib.rs

export type Item = {
  /** unique in the library: the key, numbered when several .bib files have it */
  id: string;
  key: string;
  kind: string;
  title: string;
  authors: string[];
  year: number | null;
  container: string | null;
  doi: string | null;
  url: string | null;
  keywords: string[];
  files: string[];
  /** the .bib file of the entry */
  bib: string;
  subtitle: string;
};

/** a piece of displayed text, marked where the search matched */
export type Span = { text: string; mark: boolean };

/** mirrors papers_core::Hit */
export type Hit = {
  item: Item;
  title: Span[];
  subtitle: Span[];
  /** for each author, whether the search matched it */
  authors: boolean[];
};

/** mirrors papers_core::YearCount */
export type YearCount = { year: number; count: number; selected: boolean };

/** mirrors papers_core::Found: the first hits, newest first, of `total`, and
 *  the matches of each year before the year filters */
export type Found = { hits: Hit[]; total: number; years: YearCount[] };

export type Status =
  | { state: "loading" }
  | { state: "ready"; count: number }
  | { state: "failed"; error: string };

/** mirrors papers_core::Row: an item as a row of the table of the main window */
export type Row = {
  id: string;
  key: string;
  /** bibtex entry type, e.g. "article" */
  kind: string;
  /** family names: "Robinson", "Robinson & Calov", "Robinson et al." */
  authors: string;
  year: number | null;
  title: string;
  container: string | null;
  tags: string[];
  doi: string | null;
  /** whether the item links a file, and whether the files are there */
  file: "none" | "found" | "missing";
  bib: string;
};

/** mirrors papers_core::TagCount */
export type TagCount = { tag: string; count: number };
