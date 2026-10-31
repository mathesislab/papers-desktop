// The library tab shows the current set: the papers of a library, in a view,
// with some tags, that match the search. Pure functions on rows, so the
// combination can be tested without the window (scripts/filters.test.ts).

import type { Row, TagCount } from "../types.ts";
import { anyOf } from "../query.ts";
import { foldTag, movedTag, parentsOf, sameTag } from "../tags.ts";

export type View = "all" | "recent" | "added" | "nopdf" | "broken" | "duplicates";

/** The four filters of the library tab; what the window keeps of it between sessions. */
export type Filters = {
  /** the .bib files whose papers are shown; none: all libraries */
  bibs: string[];
  view: View;
  /** the tags a paper must have, as groups of alternatives: a tag of each group (those under a tag count);
   *  clicked in the sidebar, a chip per group */
  tags: string[][];
  /** the text of the search field, in the syntax of the panel (a typed `tag:` is part of it) */
  query: string;
};

export const NO_FILTERS: Filters = { bibs: [], view: "all", tags: [], query: "" };

export const VIEWS: { view: View; name: string }[] = [
  { view: "all", name: "All papers" },
  { view: "recent", name: "Recently opened" },
  { view: "added", name: "Recently added" },
  { view: "nopdf", name: "No PDF" },
  { view: "broken", name: "Broken file links" },
  { view: "duplicates", name: "Possible duplicates" },
];

const viewName = (view: View) => VIEWS.find((v) => v.view === view)?.name ?? "";

/** How many papers *Recently added* shows: the last ones added to the library chosen. */
export const ADDED_SHOWN = 100;

/** The papers of the views that the app gives in their order (ids). */
export type Lists = {
  /** the papers opened last, the last first */
  recent: string[];
  /** the papers with a date added, the newest first */
  added: string[];
  /** the groups of papers that may be the same */
  duplicates: string[][];
};

export const NO_LISTS: Lists = { recent: [], added: [], duplicates: [] };

/** Whether a paper of a .bib file is in the libraries chosen (none: all libraries). */
const inLibraries = (bibs: string[], bib: string) => bibs.length === 0 || bibs.includes(bib);

/** Those of `ids` within the libraries (none: all libraries). */
function within(rows: Row[], ids: string[], bibs: string[]): string[] {
  const known = new Set(rows.filter((r) => inLibraries(bibs, r.bib)).map((r) => r.id));
  return ids.filter((id) => known.has(id));
}

/** The papers *Recently added* shows, before tags and search: the last `ADDED_SHOWN` added to the libraries. */
const lastAdded = (rows: Row[], added: string[], bibs: string[]) => within(rows, added, bibs).slice(0, ADDED_SHOWN);

export const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

const text = (v: unknown): v is string => typeof v === "string";

/** Filters as saved, any part missing or of a wrong type made the default; a .bib no longer in the library (`bibs`)
 *  is dropped. Those of an older version: a single `bib`, and `tags` of one tag each. */
export function restored(saved: (Partial<Filters> & { bib?: unknown }) | null | undefined, bibs?: string[]): Filters {
  const chosen = Array.isArray(saved?.bibs) ? saved.bibs.filter(text) : text(saved?.bib) ? [saved.bib] : [];
  const groups = Array.isArray(saved?.tags) ? (saved.tags as unknown[]) : [];
  const tags = groups
    .map((g) => (text(g) ? [g] : Array.isArray(g) ? g.filter(text) : []))
    .filter((g) => g.length > 0);
  return {
    bibs: [...new Set(chosen)].filter((b) => !bibs || bibs.includes(b)),
    view: VIEWS.find((v) => v.view === saved?.view)?.view ?? "all",
    tags,
    query: text(saved?.query) ? saved.query : "",
  };
}

/** The query the core matches: the search text, and a `tag:` term per group of tag filters (any of its tags), all
 *  of which must hold. */
export function searchOf(f: Filters): string {
  return [f.query.trim(), ...f.tags.map((group) => anyOf("tag", group))].filter(Boolean).join(" ");
}

/** A library clicked in the sidebar (none: *All libraries*): that one alone; with ⌘ (`toggle`), it is added to the
 *  libraries chosen or taken out of them. */
export function chooseLibrary(bibs: string[], bib: string | null, toggle: boolean): string[] {
  if (bib === null) return [];
  if (!toggle) return [bib];
  return bibs.includes(bib) ? bibs.filter((b) => b !== bib) : [...bibs, bib];
}

/**
 * The papers of the current set. `matched` are the ids the core found for `searchOf(f)` (none yet: no papers); the
 * library and the view narrow them. Recent papers come in the order they were opened, the papers added newest first
 * (the last `ADDED_SHOWN` of the library, then the tags and search), duplicates in their groups, the rest in the order
 * of `rows`.
 */
export function currentSet(rows: Row[], matched: Set<string> | null, f: Filters, lists: Lists): Row[] {
  if (!matched) return [];
  const keep = (r: Row) => matched.has(r.id) && inLibraries(f.bibs, r.bib);
  const pick = (ids: string[]) => {
    const byId = new Map(rows.map((r) => [r.id, r]));
    return ids.map((id) => byId.get(id)).filter((r): r is Row => r !== undefined && keep(r));
  };
  switch (f.view) {
    case "recent":
      return pick(lists.recent);
    case "added":
      return pick(lastAdded(rows, lists.added, f.bibs));
    case "duplicates":
      return pick(lists.duplicates.flat());
    case "nopdf":
      return rows.filter((r) => keep(r) && r.file === "none");
    case "broken":
      return rows.filter((r) => keep(r) && r.file === "missing");
    default:
      return rows.filter(keep);
  }
}

/** The papers of each view within the libraries (none: all libraries), before tags and search. */
export function viewCounts(rows: Row[], lists: Lists, bibs: string[]): Record<View, number> {
  const inBib = rows.filter((r) => inLibraries(bibs, r.bib));
  const files = (state: Row["file"]) => inBib.filter((r) => r.file === state).length;
  return {
    all: inBib.length,
    recent: within(rows, lists.recent, bibs).length,
    added: lastAdded(rows, lists.added, bibs).length,
    nopdf: files("none"),
    broken: files("missing"),
    duplicates: within(rows, lists.duplicates.flat(), bibs).length,
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
export const hasTag = (tags: string[][], tag: string) => tags.some((group) => group.some((t) => sameTag(t, tag)));

/** The groups without a tag, an empty group left out. */
const dropTag = (tags: string[][], tag: string) =>
  tags.map((group) => group.filter((t) => !sameTag(t, tag))).filter((group) => group.length > 0);

/** Click on a tag: filter it too (a group of its own: a paper must have it as well), or not any more. With ⌘
 *  (`alternative`), it is added as an alternative to the last group (a paper may have it instead), a group of its own
 *  when there is none; or taken out. */
export function toggleTag(tags: string[][], tag: string, alternative = false): string[][] {
  if (hasTag(tags, tag)) return dropTag(tags, tag);
  if (!alternative || !tags.length) return [...tags, [tag]];
  return [...tags.slice(0, -1), [...tags[tags.length - 1], tag]];
}

/** The same tags in any order and spelling. */
const sameGroup = (a: string[], b: string[]) =>
  a.length === b.length && a.every((t) => b.some((u) => sameTag(t, u)));

/** A tag renamed: the filters on it, and on the tags under it, follow (twice the same tag in a group, or twice the
 *  same group: once). */
export function renameTags(tags: string[][], old: string, name: string): string[][] {
  const groups = tags.map((group) => {
    const moved = group.map((t) => movedTag(t, old, name));
    return moved.filter((t, i) => moved.findIndex((u) => sameTag(u, t)) === i);
  });
  return groups.filter((g, i) => groups.findIndex((h) => sameGroup(g, h)) === i);
}

/** A filter shown as a chip above the table; the × takes it off: `group`, the index of a group of tags. */
export type Chip = { kind: "bib" | "view" | "tag" | "search"; text: string; group?: number };

/** A chip of several values: `Library: a.bib, b.bib`; one value alone. */
export const several = (name: string, values: string[]) =>
  values.length > 1 ? `${name}: ${values.join(", ")}` : (values[0] ?? "");

/** The chips of the filters that are on, in the order libraries › view › tags › search. */
export function chips(f: Filters): Chip[] {
  const list: Chip[] = [];
  if (f.bibs.length) list.push({ kind: "bib", text: several("Library", f.bibs.map(fileName)) });
  if (f.view !== "all") list.push({ kind: "view", text: viewName(f.view) });
  f.tags.forEach((group, i) => list.push({ kind: "tag", text: several("Tag", group), group: i }));
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
      return { ...f, bibs: [] };
    case "view":
      return { ...f, view: "all" };
    case "tag":
      return { ...f, tags: f.tags.filter((_, i) => i !== chip.group) };
    case "search":
      return { ...f, query: "" };
  }
}
