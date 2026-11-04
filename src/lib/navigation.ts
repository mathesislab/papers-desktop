// Back and forward in the main window, as a browser's: the places it showed, the tab in front and, in the Papers
// tab, the PDF in front. A selection, an item read in place of a list are no places. Pure functions,
// tested by scripts/navigation.test.ts (npm test).

import type { Tab } from "./layout.ts";

/** A place of the window: a tab in front; in the Papers tab, the PDF of a paper (its id); else none. */
export interface Place {
  tab: Tab;
  paper: string | null;
}

/** The places before the one shown, the last the nearest, and after it, the first the nearest. */
export interface History {
  back: Place[];
  forward: Place[];
}

export const NO_HISTORY: History = { back: [], forward: [] };

/** the places kept back */
export const HISTORY_MAX = 50;

export const samePlace = (a: Place, b: Place) => a.tab === b.tab && a.paper === b.paper;

/** The place shown by the tab in front and the PDF in front of the Papers tab: another tab's is the tab alone. */
export const placeOf = (tab: Tab, active: string | null): Place => ({ tab, paper: tab === "papers" ? active : null });

/** Another place reached from `from`: it goes back, the places forward are dropped. */
export function went(h: History, from: Place, to: Place): History {
  if (samePlace(from, to)) return h;
  return { back: [...h.back, from].slice(-HISTORY_MAX), forward: [] };
}

/** Back from `current`: the place to show and the history then; none at the start. */
export function goBack(h: History, current: Place): { to: Place; history: History } | null {
  const to = h.back.at(-1);
  return to ? { to, history: { back: h.back.slice(0, -1), forward: [current, ...h.forward] } } : null;
}

/** Forward from `current`: the place to show and the history then; none at the end. */
export function goForward(h: History, current: Place): { to: Place; history: History } | null {
  const [to, ...rest] = h.forward;
  return to ? { to, history: { back: [...h.back, current], forward: rest } } : null;
}

/** The history with the papers open only (`open`): the places of those closed dropped, and of the places left side
 *  by side, the same once; none the same as the place shown (`current`) next to it. */
export function kept(h: History, open: string[], current: Place): History {
  const live = (p: Place) => p.paper === null || open.includes(p.paper);
  const once = (places: Place[], edge: Place) =>
    places.filter((p, i) => !samePlace(p, places[i + 1] ?? edge));
  const back = once(h.back.filter(live), current);
  const forward = once([...h.forward.filter(live)].reverse(), current).reverse();
  return { back, forward };
}

/** What a place is called: the tab, a paper by its title (`titleOf`). */
export function placeText(p: Place, titleOf: (id: string) => string | undefined): string {
  if (p.tab === "library") return "the Library";
  if (p.tab !== "papers") return p.tab === "feeds" ? "Feeds" : "Search";
  if (p.paper === null) return "a paper";
  const title = titleOf(p.paper);
  return title ? `“${title.length > 60 ? title.slice(0, 59) + "…" : title}”` : "a paper";
}
