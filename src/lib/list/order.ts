// The rows of a list in their order, as a selection moves through them: a range, a step of the keys, the row after
// some go. Pure functions of ids, for the Library, Feeds and Search tabs (`selection.svelte.ts`), tested by
// scripts/list.test.ts (npm test).

/** The rows from one to another, in the order of the list (one alone if either is not in it). */
export function rangeOf(ids: string[], from: string, to: string): string[] {
  const [a, b] = [ids.indexOf(from), ids.indexOf(to)].sort((x, y) => x - y);
  return a < 0 ? [to] : ids.slice(a, b + 1);
}

/** Where a key moves the focus: some rows on from it (from the first when it is not in the list), or to the first
 *  or the last, the ends kept to; none in an empty list. */
export function stepped(ids: string[], focus: string | null, to: number | "first" | "last"): string | null {
  if (!ids.length) return null;
  const at = focus === null ? -1 : ids.indexOf(focus);
  const index = to === "first" ? 0 : to === "last" ? ids.length - 1 : at < 0 ? 0 : Math.max(0, Math.min(ids.length - 1, at + to));
  return ids[index];
}

/** The row to show after some go from the list: the first after the last of them that stays, else the last before;
 *  none when no row stays. */
export function nextAfter(ids: string[], gone: Set<string>): string | null {
  const last = ids.reduce((at, id, i) => (gone.has(id) ? i : at), -1);
  if (last < 0) return null;
  return ids.slice(last + 1).find((id) => !gone.has(id)) ?? ids.slice(0, last).reverse().find((id) => !gone.has(id)) ?? null;
}
