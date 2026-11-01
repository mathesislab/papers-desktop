// The columns of the tables of the three tabs (the library's Table, FeedTable; their headers are TableHead): those
// shown, in their order, each with its width; the one that takes the room left (the title, else the last), whose
// width is its least; each column's least width; how an edge of a header resizes, how a header dragged moves its
// column, the menu that shows or hides one, the layout of an older version read. Pure functions, tested by
// scripts/columns.test.ts (npm test).

/** A column shown, and its width in pixels (the flexible column's: its least). */
export type Shown<K extends string = string> = { key: K; width: number };

/** A column a table can show: its name in the header and the menu, its width at first, its least. */
export interface Spec {
  name: string;
  width: number;
  /** none: `LEAST` */
  least?: number;
}

export type Specs<K extends string> = Record<K, Spec>;

/** the least width of a column without its own */
export const LEAST = 30;

export const leastOf = <K extends string>(specs: Specs<K>, key: K) => specs[key].least ?? LEAST;

/** The widths of the columns shown, none under its least. */
export const widthsOf = <K extends string>(shown: Shown<K>[], specs: Specs<K>) =>
  shown.map((c) => Math.max(leastOf(specs, c.key), c.width));

/** The column that takes the room left: the title, else the last. */
export const flexibleOf = (shown: Shown[]) => {
  const title = shown.findIndex((c) => c.key === "title");
  return title < 0 ? shown.length - 1 : title;
};

/** The grid of a row: the columns before those shown (`lead`, fixed), then those shown, the flexible one at least
 *  its width and taking the room left. */
export function gridOf<K extends string>(lead: number[], shown: Shown<K>[], specs: Specs<K>): string {
  const widths = widthsOf(shown, specs);
  const flexible = flexibleOf(shown);
  return [...lead.map((w) => `${w}px`), ...widths.map((w, i) => (i === flexible ? `minmax(${w}px, 1fr)` : `${w}px`))].join(" ");
}

/** The width of a row with every column at its width: the table scrolls across under it. */
export const widthOf = <K extends string>(lead: number[], shown: Shown<K>[], specs: Specs<K>) =>
  [...lead, ...widthsOf(shown, specs)].reduce((sum, w) => sum + w, 0);

/**
 * The edge after column `index` dragged `moved` pixels from where it was pressed, the columns being `shown` then
 * and drawn `drawn` wide (the flexible one as wide as it was drawn) and the flexible one filled the room (`filled`): the edge
 * follows the pointer, the other columns keep their widths. Before the flexible column, after the last, or with the
 * flexible one at its least, the edge resizes the column before it (the flexible one takes the room left); after
 * the flexible column, which fills the room, it resizes the column after it, the flexible one giving the room or
 * taking it (its least going down with it).
 */
export function resized<K extends string>(
  shown: Shown<K>[],
  specs: Specs<K>,
  drawn: number[],
  index: number,
  moved: number,
  filled: boolean,
): Shown<K>[] {
  const flexible = flexibleOf(shown);
  const least = (i: number) => leastOf(specs, shown[i].key);
  const out = shown.map((c) => ({ ...c }));
  if (index < flexible || index === shown.length - 1 || !filled) {
    out[index].width = Math.max(least(index), drawn[index] + moved);
    return out;
  }
  // as far as the column after the edge and the flexible one allow
  const change = Math.min(drawn[index + 1] - least(index + 1), Math.max(least(flexible) - drawn[flexible], moved));
  out[index + 1].width = drawn[index + 1] - change;
  out[flexible].width = Math.min(widthsOf(shown, specs)[flexible], drawn[flexible] + change);
  return out;
}

/** The column at `from` moved before the column at `to` (the end: `shown.length`); the same when it would not move. */
export function moved<K extends string>(shown: Shown<K>[], from: number, to: number): Shown<K>[] {
  if (to === from || to === from + 1) return shown;
  const out = [...shown];
  const [column] = out.splice(from, 1);
  out.splice(to > from ? to - 1 : to, 0, column);
  return out;
}

/** A column of the menu shown, at the end at its width at first, or hidden; the last one shown stays. */
export function toggled<K extends string>(shown: Shown<K>[], specs: Specs<K>, key: K): Shown<K>[] {
  if (!shown.some((c) => c.key === key)) return [...shown, { key, width: specs[key].width }];
  return shown.length > 1 ? shown.filter((c) => c.key !== key) : shown;
}

/** The columns shown at first: `keys`, at their widths at first. */
export const columnsOf = <K extends string>(keys: K[], specs: Specs<K>): Shown<K>[] =>
  keys.map((key) => ({ key, width: specs[key].width }));

/**
 * The columns as the layout saved them, those not of the table left out (none left: `defaults`). An older version
 * saved a list of `{column, width}` (the library), or the widths by column in a fixed order (the Feeds and Search
 * tabs: `defaults`, each at its saved width).
 */
export function restoredColumns<K extends string>(saved: unknown, specs: Specs<K>, defaults: K[]): Shown<K>[] {
  const known = (k: unknown): k is K => typeof k === "string" && Object.hasOwn(specs, k);
  const width = (w: unknown, key: K) => (typeof w === "number" && Number.isFinite(w) && w > 0 ? Math.round(w) : specs[key].width);
  if (Array.isArray(saved)) {
    const out: Shown<K>[] = [];
    for (const c of saved) {
      const key = c && typeof c === "object" ? ((c as { key?: unknown }).key ?? (c as { column?: unknown }).column) : null;
      if (known(key) && !out.some((o) => o.key === key)) out.push({ key, width: width((c as { width?: unknown }).width, key) });
    }
    if (out.length) return out;
  } else if (saved && typeof saved === "object") {
    const widths = saved as Record<string, unknown>;
    return defaults.map((key) => ({ key, width: width(widths[key], key) }));
  }
  return columnsOf(defaults, specs);
}
