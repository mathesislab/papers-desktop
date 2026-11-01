// The actions on the items of the Feeds and Search tabs, each once: the keys, the row menu, the selection bar, the
// buttons of the details and Later ▾ read them from here, so that each says its name one way (the menus in title
// case, `titled`) in both tabs. Those of both (`itemActions`: open, add, star) and the Feeds tab's own
// (`feedActions`). What they run is the tab's (`Doers`); their names and when they apply are pure, tested by
// scripts/actions.test.ts (npm test).

import type { ItemKind, Reminder, Triage } from "./api.ts";
import { isPost } from "./kinds.ts";
import { starrable, toggled, type TriageAction } from "./items.ts";

/** What an action knows of an item: a row of the list, or the details of one (a linked paper not in the list). */
export type Item = { id: string; kind: ItemKind; triage: Triage; reminder?: Reminder | null; url?: string | null; doi?: string | null };

/** the actions of both tabs */
export type ItemActionName = "open" | "add" | "addBare" | "star";
export type ActionName = ItemActionName | "find" | "read" | "dismiss" | "keepUnread" | "later" | "clearReminder";

export interface Action {
  /** the key that does it to the selection; none: the menus and the buttons only */
  key: string | null;
  /** what it is called for the items it acts on, in sentence case: a toggle says which way it goes */
  label: (items: Item[]) => string;
  /** whether it can act on them */
  enabled: (items: Item[]) => boolean;
  /** what a button of it says on hover */
  hint?: string;
  /** done to the items of those ids */
  run: (ids: string[]) => unknown;
}

/** What the actions of both tabs do: the tab's. */
export interface ItemDoers {
  open: (ids: string[]) => unknown;
  add: (ids: string[], pdf: boolean) => unknown;
  triage: (action: TriageAction, ids: string[]) => unknown;
}

/** What the actions of the Feeds tab do. */
export interface Doers extends ItemDoers {
  /** the papers behind a post looked for again */
  find: (id: string) => unknown;
  /** the item in focus */
  focus: () => string | null;
  keepUnread: (ids: string[]) => unknown;
  /** Later ▾: kept unread, snoozed, the reminder cleared */
  later: (ids: string[]) => unknown;
  clearReminder: (ids: string[]) => unknown;
}

/** the snoozes of Later ▾ and the row menu, in days: till the start of that day */
export const SNOOZES: { text: string; days: number }[] = [
  { text: "1 day", days: 1 },
  { text: "3 days", days: 3 },
  { text: "1 week", days: 7 },
  { text: "1 month", days: 30 },
];

const always = () => true;
const all = (items: Item[], triage: Triage) => items.length > 0 && items.every((i) => i.triage === triage);

/** The actions of both tabs, by name. */
export function itemActions(d: ItemDoers): Record<ItemActionName, Action> {
  return {
    open: {
      key: "o",
      label: (items) => (items.length === 1 && isPost(items[0].kind) ? "Open post" : "Open"),
      // a post by its link, a paper by its link or its DOI; a row does not say them: they are looked for when opened
      enabled: (items) => items.some((i) => i.url === undefined || !!i.url || (!isPost(i.kind) && !!i.doi)),
      hint: "Open its page in the browser",
      run: d.open,
    },
    add: {
      key: "a",
      label: () => "Add…",
      enabled: always,
      hint: "Add to the library, with the free PDF if there is one",
      run: (ids) => d.add(ids, true),
    },
    addBare: {
      key: null,
      label: () => "Add without PDF…",
      enabled: always,
      hint: "Add to the library without looking for its PDF",
      run: (ids) => d.add(ids, false),
    },
    star: {
      key: "s",
      label: (items) => (all(items.filter(starrable), "starred") ? "Unstar" : "Star"),
      enabled: (items) => toggled(items, "star") !== null,
      hint: "Keep it, to come back to it",
      run: (ids) => d.triage("star", ids),
    },
  };
}

/** The actions of the Feeds tab, by name: those of both, and the triage. */
export function feedActions(d: Doers): Record<ActionName, Action> {
  return {
    ...itemActions(d),
    find: {
      key: "f",
      label: () => "Find linked paper",
      enabled: (items) => items.length === 1 && isPost(items[0].kind),
      // the item in focus, when it is one of them
      run: (ids) => {
        const focus = d.focus();
        if (ids.length) d.find(focus !== null && ids.includes(focus) ? focus : ids[0]);
      },
    },
    read: {
      key: "m",
      label: (items) => (items.some((i) => i.triage === "new") ? "Mark read" : "Mark unread"),
      enabled: (items) => toggled(items, "read") !== null,
      run: (ids) => d.triage("read", ids),
    },
    dismiss: {
      key: "x",
      label: (items) => (all(items, "dismissed") ? "Restore" : "Dismiss"),
      enabled: (items) => toggled(items, "dismiss") !== null,
      run: (ids) => d.triage("dismiss", ids),
    },
    keepUnread: {
      key: "u",
      label: () => "Keep unread",
      enabled: always,
      run: d.keepUnread,
    },
    later: {
      key: "z",
      label: () => "Later",
      enabled: always,
      hint: "Keep it unread (U), or snooze it",
      run: d.later,
    },
    clearReminder: {
      key: null,
      label: () => "Clear reminder",
      enabled: (items) => items.some((i) => !!i.reminder),
      run: d.clearReminder,
    },
  };
}

/** the words a title leaves in lower case: articles, conjunctions, prepositions of four letters or fewer */
const SMALL = new Set(["a", "an", "the", "and", "or", "as", "at", "by", "for", "from", "in", "into", "of", "on", "to", "via", "with"]);

/** A label in title case, as a menu says it (`Mark all above as read` → `Mark All Above as Read`). */
export const titled = (label: string) =>
  label.replace(/\S+/g, (word, at: number) => (at > 0 && SMALL.has(word) ? word : word[0].toUpperCase() + word.slice(1)));

/** An action as a button says it: the star and the clock before it, ↗ after what opens a page, ▾ after a menu. */
export function buttonText(name: ActionName, label: string): string {
  if (name === "star") return `${label === "Unstar" ? "★" : "☆"} ${label}`;
  if (name === "later") return `⏰ ${label} ▾`;
  if (name === "open") return `${label} ↗`;
  return label;
}
