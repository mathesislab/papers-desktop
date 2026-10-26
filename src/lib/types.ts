// Mirrors papers_core::Item and the Status of src-tauri/src/lib.rs

export type Item = {
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

/** mirrors papers_core::Found: the first hits, newest first, of `total` */
export type Found = { hits: Hit[]; total: number };

export type Status =
  | { state: "loading" }
  | { state: "ready"; count: number }
  | { state: "failed"; error: string };
