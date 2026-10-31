// Saved searches, in both tabs: named searches in the order of the sidebar, a name once in any case. The library
// keeps its filters (settings.json), the Feeds tab a query (the feeds' config.json). Pure functions, tested by
// scripts/saved.test.ts (npm test).

export type Named = { name: string };

/** Whether two names are the same, in any case. */
export const sameName = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

/** Why a name cannot be given to a search (that of `own`, being renamed, aside); none: it can. */
export function refusal(list: Named[], name: string, own?: string): string | null {
  if (!name.trim()) return "A name is needed";
  const taken = list.some((s) => sameName(s.name, name) && !(own !== undefined && sameName(s.name, own)));
  return taken ? "A saved search has that name" : null;
}

/** A search saved, at the end, its name trimmed. */
export const added = <T extends Named>(list: T[], search: T): T[] => [...list, { ...search, name: search.name.trim() }];

/** The search of a name changed by `change` (renamed, updated), in its place. */
export const changed = <T extends Named>(list: T[], name: string, change: (search: T) => T): T[] =>
  list.map((s) => (sameName(s.name, name) ? change(s) : s));

/** A search renamed, the new name trimmed. */
export const renamed = <T extends Named>(list: T[], name: string, to: string): T[] =>
  changed(list, name, (s) => ({ ...s, name: to.trim() }));

/** Without the search of a name. */
export const removed = <T extends Named>(list: T[], name: string): T[] => list.filter((s) => !sameName(s.name, name));
