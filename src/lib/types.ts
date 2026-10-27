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

/** mirrors papers_core::TagCount: a tag of the library, or a parent of one ("climate" for "climate/amoc") */
export type TagCount = {
  tag: string;
  /** the papers that have the tag itself; 0 for a parent no paper has */
  count: number;
  /** the papers that have the tag or a tag under it, each paper once */
  under: number;
};

/** mirrors writer::Writer of src-tauri: how a .bib file is written */
export type Writer = {
  bib: string;
  /** "papers": by the papers command; "native": by the app; "papers-missing":
   *  a papers library, read-only while papers 3.0.0b4 or later is not found */
  kind: "papers" | "native" | "papers-missing";
  /** of the papers command found, for a papers library: too old when missing */
  version: string | null;
};

/** mirrors papers_core::Added: the keys of the entries given to add */
export type Added = {
  added: string[];
  /** already in the library: the same key, or for papers the same paper */
  existing: string[];
};

/** mirrors writer::WriteError: what a write is rejected with */
export type WriteError = {
  /** "papers-missing": a papers library without papers 3.0.0b4 or later; "changed": the .bib
   *  changed since it was read, and is read again: try again */
  kind: "papers-missing" | "changed" | "failed";
  message: string;
};
