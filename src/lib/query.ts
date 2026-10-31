// Edit the filters of a query: the years clicked in the panel. The tags of the
// main window are filters of their own (library/filters.ts); a `tag:` typed in
// its search is text.

import { movedTag } from "./tags.ts";

/** a year filter that keeps years (y:2012, year:2010-2015), not one that excludes them */
const KEPT = /(^|\s)(?:y|year):(\S*)/gi;

const excluded = (year: number) => new RegExp(`(^|\\s)-(?:y|year):${year}(?=\\s|$)`, "gi");

const tidy = (query: string) => query.replace(/\s+/g, " ").trim();

/** The years of the kept filter, when it is a single year or range. */
function keptRange(query: string): [number, number] | null {
  const filters = [...query.matchAll(KEPT)];
  const range = filters.length === 1 ? /^(\d{4})(?:-(\d{4}))?$/.exec(filters[0][2]) : null;
  return range ? [Number(range[1]), Number(range[2] ?? range[1])] : null;
}

/** Keep the years from..to only, in place of the kept year filters. */
export function onlyYears(query: string, from: number, to = from): string {
  const [a, b] = [Math.min(from, to), Math.max(from, to)];
  return tidy(`${query.replace(KEPT, " ")} y:${a === b ? a : `${a}-${b}`}`);
}

/** Click: that year only, or no year filter when it was so already. */
export function toggleYear(query: string, year: number): string {
  const range = keptRange(query);
  if (range && range[0] === year && range[1] === year) return tidy(query.replace(KEPT, " "));
  return onlyYears(query, year);
}

/** Shift-click: stretch the kept range to the year. */
export function extendYears(query: string, year: number): string {
  const range = keptRange(query);
  return range ? onlyYears(query, Math.min(range[0], year), Math.max(range[1], year)) : onlyYears(query, year);
}

/** All years again: no year filter, kept or excluded. */
export function clearYears(query: string): string {
  return tidy(query.replace(/(^|\s)-?(?:y|year):\S*/gi, " "));
}

/** Alt-click: exclude the year, or take it back. */
export function toggleExcluded(query: string, year: number): string {
  if (excluded(year).test(query)) return tidy(query.replace(excluded(year), " "));
  return tidy(`${query} -y:${year}`);
}

/** A field filter that takes any of the values: `anyOf("src", ["tc", "Nature Geosci"])` is `src:tc,"Nature Geosci"`;
 *  a value with a space or a comma is quoted, a quote dropped (a filter cannot have one), an empty value left out.
 *  None: no filter, "". */
export function anyOf(name: string, values: string[]): string {
  const written = values
    .map((v) => v.replace(/"/g, ""))
    .filter((v) => v.trim())
    .map((v) => (/[\s,]/.test(v) ? `"${v}"` : v));
  return written.length ? `${name}:${written.join(",")}` : "";
}

/** The values of a field filter, its alternatives between commas: `tc,"Nature Geosci"` is tc and Nature Geosci. */
export const alternatives = (values: string) =>
  [...values.matchAll(/(?:^|,)(?:"([^"]*)"|([^,]*))/g)].map((m) => m[1] ?? m[2]);

/** The filter of a tag: tag:amoc, or tag:"mass balance". */
export const tagTerm = (tag: string) => anyOf("tag", [tag]);

/** A filter on tags, negated or not: tag:amoc, -tag:amoc, tag:"mass balance", tag:amoc,ice. */
const TAG_TERM = /(^|\s)(-?)tag:((?:"[^"]*"|[^\s,]*)(?:,(?:"[^"]*"|[^\s,]*))*)(?=\s|$)/gi;

/** A tag renamed: the filters in the query on it, and on the tags under it, follow. */
export function renameTagFilter(query: string, old: string, name: string): string {
  return tidy(
    query.replace(TAG_TERM, (term, space, minus, values) => {
      const tags = alternatives(values);
      const moved = tags.map((tag) => movedTag(tag, old, name));
      return moved.every((tag, i) => tag === tags[i]) ? term : `${space}${minus}${anyOf("tag", moved)}`;
    }),
  );
}
