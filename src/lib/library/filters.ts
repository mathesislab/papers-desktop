// The library tab shows the current set: the papers of a library, in a view,
// with some tags, that match the search. Pure functions on rows, so the
// combination can be tested without the window (scripts/filters.test.ts).

import type { Row, TagCount } from "../types.ts";
import { tagTerm } from "../query.ts";
import { foldTag, movedTag, parentsOf, sameTag } from "../tags.ts";

export type View = "all" | "recent" | "nopdf" | "broken" | "duplicates";

/** The four filters of the library tab; what the window keeps of it between sessions. */
export type Filters = {
  /** the .bib file whose papers are shown; none: all libraries */
  bib: string | null;
  view: View;
  /** the tags a paper must all have (those under a tag count); clicked in the sidebar, shown as chips */
  tags: string[];
  /** the text of the search field, in the syntax of the panel (a typed `tag:` is part of it) */
  query: string;
};

export const NO_FILTERS: Filters = { bib: null, view: "all", tags: [], query: "" };

export const VIEWS: { view: View; name: string }[] = [
  { view: "all", name: "All papers" },
  { view: "recent", name: "Recently opened" },
  { view: "nopdf", name: "No PDF" },
  { view: "broken", name: "Broken file links" },
  { view: "duplicates", name: "Possible duplicates" },
];

const viewName = (view: View) => VIEWS.find((v) => v.view === view)?.name ?? "";

export const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** Filters as saved, any part missing or of a wrong type made the default; a .bib no longer in the library (`bibs`) is dropped. */
export function restored(saved: Partial<Filters> | null | undefined, bibs?: string[]): Filters {
  return {
    bib: typeof saved?.bib === "string" && (!bibs || bibs.includes(saved.bib)) ? saved.bib : null,
    view: VIEWS.find((v) => v.view === saved?.view)?.view ?? "all",
    tags: Array.isArray(saved?.tags) ? saved.tags.filter((t) => typeof t === "string") : [],
    query: typeof saved?.query === "string" ? saved.query : "",
  };
}

/** The query the core matches: the search text, and a `tag:` term per tag filter, all of which must hold. */
export function searchOf(f: Filters): string {
  return [f.query.trim(), ...f.tags.map(tagTerm)].filter(Boolean).join(" ");
}

/**
 * The papers of the current set. `matched` are the ids the core found for `searchOf(f)` (none yet: no papers); the
 * library and the view narrow them. Recent papers come in the order they were opened, duplicates in their groups,
 * the rest in the order of `rows`.
 */
export function currentSet(rows: Row[], matched: Set<string> | null, f: Filters, recent: string[], duplicates: string[][]): Row[] {
  if (!matched) return [];
  const keep = (r: Row) => matched.has(r.id) && (f.bib === null || r.bib === f.bib);
  const pick = (ids: string[]) => {
    const byId = new Map(rows.map((r) => [r.id, r]));
    return ids.map((id) => byId.get(id)).filter((r): r is Row => r !== undefined && keep(r));
  };
  switch (f.view) {
    case "recent":
      return pick(recent);
    case "duplicates":
      return pick(duplicates.flat());
    case "nopdf":
      return rows.filter((r) => keep(r) && r.file === "none");
    case "broken":
      return rows.filter((r) => keep(r) && r.file === "missing");
    default:
      return rows.filter(keep);
  }
}

/** The papers of each view within a library (none: all libraries), before tags and search. */
export function viewCounts(rows: Row[], recent: string[], duplicates: string[][], bib: string | null): Record<View, number> {
  const inBib = new Set(rows.filter((r) => bib === null || r.bib === bib).map((r) => r.id));
  const within = (ids: string[]) => ids.filter((id) => inBib.has(id)).length;
  const files = (state: Row["file"]) => rows.filter((r) => inBib.has(r.id) && r.file === state).length;
  return {
    all: inBib.size,
    recent: within(recent),
    nopdf: files("none"),
    broken: files("missing"),
    duplicates: within(duplicates.flat()),
  };
}

/** The .bib files of the rows, with their number of papers, in the order they first come. */
export function libraryCounts(rows: Row[]): { bib: string; count: number }[] {
  const counts = new Map<string, number>();
  for (const r of rows) counts.set(r.bib, (counts.get(r.bib) ?? 0) + 1);
  return [...counts].map(([bib, count]) => ({ bib, count }));
}

/**
 * The tags of some rows with their counts, as the core counts the tags of a library (`Library::tags`): a tag in any
 * spelling is one, named as written first; a parent no paper has comes with a count of 0; a tag before the tags under
 * it, in the order of their parts. `rows` are newest first, as the core gives them.
 */
export function tagCounts(rows: Row[]): TagCount[] {
  const tags = new Map<string, TagCount>();
  const entry = (tag: string) => {
    let t = tags.get(foldTag(tag));
    if (!t) tags.set(foldTag(tag), (t = { tag, count: 0, under: 0 }));
    return t;
  };
  for (const r of rows) {
    const seen = new Set<string>();
    for (const tag of r.tags) {
      if (seen.has(foldTag(tag))) continue;
      seen.add(foldTag(tag));
      entry(tag).count++;
    }
  }
  // after the tags written, whose spelling a parent must not take
  for (const r of rows) {
    const seen = new Set<string>();
    for (const tag of r.tags) {
      for (const name of [...parentsOf(tag), tag]) {
        if (seen.has(foldTag(name))) continue;
        seen.add(foldTag(name));
        entry(name).under++;
      }
    }
  }
  const parts = (t: TagCount) => foldTag(t.tag).split("/");
  return [...tags.values()].sort((a, b) => {
    const [x, y] = [parts(a), parts(b)];
    for (let i = 0; i < Math.min(x.length, y.length); i++) if (x[i] !== y[i]) return x[i] < y[i] ? -1 : 1;
    return x.length - y.length;
  });
}

/** Whether a tag is one of the tag filters. */
export const hasTag = (tags: string[], tag: string) => tags.some((t) => sameTag(t, tag));

/** Click on a tag: filter it too, or not any more. */
export const toggleTag = (tags: string[], tag: string) => (hasTag(tags, tag) ? tags.filter((t) => !sameTag(t, tag)) : [...tags, tag]);

/** A tag renamed: the filters on it, and on the tags under it, follow (twice the same tag: once). */
export function renameTags(tags: string[], old: string, name: string): string[] {
  const moved = tags.map((t) => movedTag(t, old, name));
  return moved.filter((t, i) => moved.findIndex((u) => sameTag(u, t)) === i);
}

/** A filter shown as a chip above the table; the × takes it off. */
export type Chip = { kind: "bib" | "view" | "tag" | "search"; text: string; tag?: string };

/** The chips of the filters that are on, in the order library › view › tags › search. */
export function chips(f: Filters): Chip[] {
  const list: Chip[] = [];
  if (f.bib !== null) list.push({ kind: "bib", text: fileName(f.bib) });
  if (f.view !== "all") list.push({ kind: "view", text: viewName(f.view) });
  for (const tag of f.tags) list.push({ kind: "tag", text: tag, tag });
  if (f.query.trim()) list.push({ kind: "search", text: `"${f.query.trim()}"` });
  return list;
}

/** The filters the library tab shows, in a few words: `papers.bib · No PDF · amoc · "ice"`; empty with no filter on. */
export function tabFilters(f: Filters): string {
  return chips(f)
    .map((c) => c.text)
    .join(" · ");
}

/** A filter taken off: its chip's ×. */
export function without(f: Filters, chip: Chip): Filters {
  switch (chip.kind) {
    case "bib":
      return { ...f, bib: null };
    case "view":
      return { ...f, view: "all" };
    case "tag":
      return { ...f, tags: f.tags.filter((t) => t !== chip.tag) };
    case "search":
      return { ...f, query: "" };
  }
}
