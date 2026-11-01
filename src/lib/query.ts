// The queries of the core as the page reads them (`queryTerms`, its tokens) and edits them: the years clicked in the
// panel, the tag filters typed in the library's search after a tag is renamed, a filter of any of some values. An
// edit takes the terms apart and puts them together again, those it leaves as they were typed (a phrase stays one:
// `"y:2010"` is no year). The tags of the main window are filters of their own (library/filters.ts); a `tag:` typed
// in its search is text.

import { movedTag } from "./tags.ts";

/** The query of some terms, one space between them. */
const joined = (terms: string[]) => terms.filter(Boolean).join(" ");

const isYear = (t: QueryTerm) => t.field === "y" || t.field === "year";
/** a year filter that keeps years (y:2012, year:2010-2015), not one that excludes them */
const kept = (t: QueryTerm) => isYear(t) && !t.negated;
/** The terms of a query of the core's own fields. */
const termsOf = (query: string) => queryTerms(query, FIELDS);

/** The years of the kept filter, when it is a single year or range. */
function keptRange(query: string): [number, number] | null {
  const filters = termsOf(query).filter(kept);
  const value = filters.length === 1 && filters[0].values.length === 1 ? filters[0].values[0] : "";
  const range = /^(\d{4})(?:-(\d{4}))?$/.exec(value);
  return range ? [Number(range[1]), Number(range[2] ?? range[1])] : null;
}

/** The query without its kept year filters. */
const withoutKept = (query: string) => termsOf(query).filter((t) => !kept(t)).map((t) => t.text);

/** Keep the years from..to only, in place of the kept year filters. */
export function onlyYears(query: string, from: number, to = from): string {
  const [a, b] = [Math.min(from, to), Math.max(from, to)];
  return joined([...withoutKept(query), `y:${a === b ? a : `${a}-${b}`}`]);
}

/** Click: that year only, or no year filter when it was so already. */
export function toggleYear(query: string, year: number): string {
  const range = keptRange(query);
  if (range && range[0] === year && range[1] === year) return joined(withoutKept(query));
  return onlyYears(query, year);
}

/** Shift-click: stretch the kept range to the year. */
export function extendYears(query: string, year: number): string {
  const range = keptRange(query);
  return range ? onlyYears(query, Math.min(range[0], year), Math.max(range[1], year)) : onlyYears(query, year);
}

/** All years again: no year filter, kept or excluded. */
export function clearYears(query: string): string {
  return joined(termsOf(query).filter((t) => !isYear(t)).map((t) => t.text));
}

/** Alt-click: exclude the year, or take it back. */
export function toggleExcluded(query: string, year: number): string {
  const terms = termsOf(query);
  const excludes = (t: QueryTerm) => isYear(t) && t.negated && t.values.length === 1 && t.values[0] === String(year);
  if (terms.some(excludes)) return joined(terms.filter((t) => !excludes(t)).map((t) => t.text));
  return joined([...terms.map((t) => t.text), `-y:${year}`]);
}

/** A field filter that takes any of the values: `anyOf("src", ["tc", "Nature Geosci"])` is `src:tc,"Nature Geosci"`;
 *  a value of other characters than letters, digits, `_`, `.` and `-` is quoted, a quote dropped (a filter cannot
 *  have one), an empty value left out. None: no filter, "". */
export function anyOf(name: string, values: string[]): string {
  const written = values
    .map((v) => v.replace(/"/g, ""))
    .filter((v) => v.trim())
    .map((v) => (/^[\w.-]+$/.test(v) ? v : `"${v}"`));
  return written.length ? `${name}:${written.join(",")}` : "";
}

/** The field names of the core's queries (crates/core/src/query.rs); a schema adds its own. */
export const FIELDS = ["author", "a", "first", "f", "title", "t", "year", "y", "journal", "j", "abstract", "abs", "key", "k", "doi", "tag", "has"];

/** A term of a query as the core reads it: its text, whether it excludes, its field (lowercase; none: a word or
 *  phrase) and its values, the alternatives of a filter (unquoted). */
export type QueryTerm = { text: string; negated: boolean; field: string | null; values: string[] };

/** The terms of a query, as the core's tokens: a word ends at a space, a "phrase" at its quote, a filter (a name of
 *  `fields` and a colon) takes values between commas; another name is part of a word. */
export function queryTerms(query: string, fields: readonly string[]): QueryTerm[] {
  const terms: QueryTerm[] = [];
  const space = (c: string | undefined) => c !== undefined && /\s/.test(c);
  let i = 0;
  while (i < query.length) {
    if (space(query[i])) {
      i++;
      continue;
    }
    const start = i;
    const negated = query[i] === "-";
    if (negated) i++;
    const name = /^[A-Za-z]*/.exec(query.slice(i))![0];
    let field: string | null = null;
    if (query[i + name.length] === ":" && fields.includes(name.toLowerCase())) {
      field = name.toLowerCase();
      i += name.length + 1;
    }
    const values: string[] = [];
    for (;;) {
      if (query[i] === '"') {
        const close = query.indexOf('"', i + 1);
        const end = close < 0 ? query.length : close;
        values.push(query.slice(i + 1, end));
        i = Math.min(end + 1, query.length);
      } else {
        let end = i;
        while (end < query.length && !space(query[end]) && !(field && query[end] === ",")) end++;
        values.push(query.slice(i, end));
        i = end;
      }
      if (!(field && query[i] === ",")) break;
      i++;
    }
    terms.push({ text: query.slice(start, i), negated, field, values });
  }
  return terms;
}

/** A tag renamed: the filters in the query on it, and on the tags under it, follow (excluding or not, any of their
 *  values). */
export function renameTagFilter(query: string, old: string, name: string): string {
  const terms = termsOf(query).map((t) => {
    if (t.field !== "tag") return t.text;
    const moved = t.values.map((tag) => movedTag(tag, old, name));
    return moved.every((tag, i) => tag === t.values[i]) ? t.text : `${t.negated ? "-" : ""}${anyOf("tag", moved)}`;
  });
  return joined(terms);
}
