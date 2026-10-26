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

export type Status =
  | { state: "loading" }
  | { state: "ready"; count: number }
  | { state: "failed"; error: string };
