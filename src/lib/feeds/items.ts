// The items of the Feeds tab: their triage by the keys, the list kept in place while they are triaged, when an item
// shown counts as seen, what an item adds to the library, how old things are said to be. Pure, tested by
// scripts/feeds.test.ts (npm test).

import type { FeedDetail, FeedRow, FetchSummary, Linked, Reminder, Triage } from "./api.ts";
import { isPost } from "./kinds.ts";

/** the keys that change the triage of the items selected */
export type TriageAction = "star" | "dismiss" | "read";

/** Whether an item can be starred: one added to the library keeps being added. */
export const starrable = (item: { triage: Triage }) => item.triage !== "added";

/**
 * The triage a key gives the items selected, and which of them it changes. Each key toggles: `s` stars, or when
 * all are starred takes them back to seen (an added item keeps its triage); `x` dismisses, or brings back dismissed
 * ones; `m` marks new items seen, or when none is new marks the seen ones new again (a starred, dismissed or added
 * item keeps its triage). None: nothing to change.
 */
export function toggled(items: { id: string; triage: Triage }[], action: TriageAction): { ids: string[]; triage: Triage } | null {
  const all = (t: Triage, of = items) => of.length > 0 && of.every((i) => i.triage === t);
  const change = (pick: (t: Triage) => boolean, triage: Triage) => {
    const ids = items.filter((i) => pick(i.triage) && i.triage !== triage).map((i) => i.id);
    return ids.length ? { ids, triage } : null;
  };
  const kept = (t: Triage) => starrable({ triage: t });
  switch (action) {
    case "star":
      return all("starred", items.filter(starrable)) ? change(kept, "seen") : change(kept, "starred");
    case "dismiss":
      return all("dismissed") ? change(() => true, "seen") : change(() => true, "dismissed");
    case "read":
      return items.some((i) => i.triage === "new")
        ? change((t) => t === "new", "seen")
        : change((t) => t === "seen", "new");
  }
}

/** How long an item must stay selected to count as seen, ms. */
export const DWELL_MS = 800;

/**
 * Seen on dwell: a new item selected becomes seen once it has stayed selected `ms`. One passed over faster (j held
 * down, the arrows run through the list) stays new: its title went by, its details were not read. *Mark all above
 * as read* (⇧Space) is for the titles skimmed.
 */
export class Dwell {
  private timer: ReturnType<typeof setTimeout> | null = null;
  private readonly onseen: (id: string) => void;
  private readonly ms: number;

  constructor(onseen: (id: string) => void, ms = DWELL_MS) {
    this.onseen = onseen;
    this.ms = ms;
  }

  /** The item selected now (none: no item), and whether it is new: the one before, if not seen yet, was passed. */
  show(id: string | null, isNew: boolean) {
    this.stop();
    if (id === null || !isNew) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      this.onseen(id);
    }, this.ms);
  }

  stop() {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }
}

/** A row unread: new, not snoozed. */
export const unread = (r: { triage: Triage; reminder?: Reminder | null }) => r.triage === "new" && r.reminder !== "snoozed";

/** Mark all above as seen: the unread items before `focus` in the list (all of them when it is not in the list). */
export function newAbove(rows: { id: string; triage: Triage; reminder?: Reminder | null }[], focus: string | null): string[] {
  const at = focus === null ? -1 : rows.findIndex((r) => r.id === focus);
  return rows.slice(0, at < 0 ? rows.length : at).filter(unread).map((r) => r.id);
}

/** Mark read, older than: the unread items dated more than some days before now (one without a date is not). */
export function newOlderThan(
  rows: { id: string; triage: Triage; reminder?: Reminder | null; date: string | null }[],
  days: number,
  now: number,
): string[] {
  const before = now - days * 86_400_000;
  return rows.filter((r) => unread(r) && r.date !== null && Date.parse(r.date) < before).map((r) => r.id);
}

/** Snoozed for some days: till the start of that day. */
export function snoozeUntil(days: number, now: Date): Date {
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() + days);
}

/**
 * The list again, as the backend answers it (`fresh`), with the items triaged here since the filters last changed
 * (`sticky`) kept in their place when the answer lost them: an item marked seen in *Unread* stays under the cursor
 * until the filters change, so that j and k do not jump. A sticky item takes the answer's row when it has one.
 */
export function mergeRows(old: FeedRow[], fresh: FeedRow[], sticky: Set<string>): FeedRow[] {
  const answered = new Set(fresh.map((r) => r.id));
  const kept = old.filter((r) => sticky.has(r.id) && !answered.has(r.id));
  if (!kept.length) return fresh;
  const out = [...fresh];
  for (const row of kept) {
    // after the row it followed in the old list that is in the new one; else first
    const before = old.slice(0, old.indexOf(row)).reverse().find((r) => out.some((o) => o.id === r.id));
    out.splice(before ? out.findIndex((o) => o.id === before.id) + 1 : 0, 0, row);
  }
  return out;
}

/** The link of a post most likely its paper: the first of high confidence, else the first. */
export const bestLink = (links: Linked[]): Linked | null => links.find((l) => l.confidence === "high") ?? links[0] ?? null;

/**
 * What adding an item adds: a paper's DOI (else its arXiv id), and the item that becomes *added*; a post's linked
 * paper, by its DOI, and that paper's item when it is one. None: nothing to add (a post without a link).
 */
export function addTarget(d: Pick<FeedDetail, "id" | "kind" | "doi" | "arxiv" | "links">): { id: string; item: string | null } | null {
  if (isPost(d.kind)) {
    const link = bestLink(d.links);
    return link ? { id: link.doi, item: link.item } : d.doi ? { id: d.doi, item: d.id } : null;
  }
  const id = d.doi ?? d.arxiv;
  return id ? { id, item: d.id } : null;
}

/** The page of an item: its link, else its DOI's. */
export const pageOf = (d: Pick<FeedDetail, "url" | "doi">) => d.url ?? (d.doi ? `https://doi.org/${d.doi}` : null);

/** How long ago: "now", "5 min", "3 h", "2 d", then the day ("3 Oct", with the year when it is not this one). */
export function ago(date: string | null, now = Date.now()): string {
  if (!date) return "";
  const at = Date.parse(date);
  if (Number.isNaN(at)) return "";
  const minutes = Math.floor((now - at) / 60000);
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes} min`;
  if (minutes < 24 * 60) return `${Math.floor(minutes / 60)} h`;
  if (minutes < 7 * 24 * 60) return `${Math.floor(minutes / (24 * 60))} d`;
  const [d, today] = [new Date(at), new Date(now)];
  const year = d.getFullYear() === today.getFullYear() ? {} : { year: "numeric" as const };
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short", ...year });
}

/** A day as the details say it: "3 Oct 2026". */
export const dayText = (date: string) =>
  new Date(date).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });

/** How long ago, in words: "just now", "8 min ago", "3 d ago", "on 3 Oct" (`ago`). */
export function since(date: string, now = Date.now()): string {
  const when = ago(date, now);
  return when === "now" ? "just now" : /^\d+ (min|h|d)$/.test(when) ? `${when} ago` : `on ${when}`;
}

/** The minutes after which opening the tab fetches again. */
export const STALE_MINUTES = 15;

/** Whether to fetch on opening the tab: never fetched, or longer ago than `minutes`. */
export function stale(last: FetchSummary | null, now = Date.now(), minutes = STALE_MINUTES): boolean {
  if (!last) return true;
  const at = Date.parse(last.at);
  return Number.isNaN(at) || now - at > minutes * 60000;
}

/** The status line of the last fetch: "fetched 8 min ago · 12 new · 3 not modified · 1 failed". */
export function fetchLine(last: FetchSummary | null, now = Date.now()): string {
  if (!last) return "not fetched yet";
  const parts = [`fetched ${since(last.at, now)}`];
  if (last.added) parts.push(`${last.added} new`);
  if (last.unchanged) parts.push(`${last.unchanged} not modified`);
  if (last.failed.length) parts.push(`${last.failed.length} failed`);
  return parts.join(" · ");
}

/** The badges of a row, after its title. */
export function badges(row: FeedRow): { text: string; kind: "lib" | "pre" | "news" | "link" | "interest" | "remind" }[] {
  const list: ReturnType<typeof badges> = [];
  if (row.reminder === "due") list.push({ text: "⏰", kind: "remind" });
  else if (row.reminder === "snoozed" && row.remind) {
    const day = new Date(row.remind).toLocaleDateString(undefined, { day: "numeric", month: "short" });
    list.push({ text: `⏰ ${day}`, kind: "remind" });
  }
  if (row.inLibrary.length) list.push({ text: "in library", kind: "lib" });
  if (row.kind === "preprint") list.push({ text: "preprint", kind: "pre" });
  else if (row.hasVersions && row.kind === "paper") list.push({ text: "has preprint", kind: "pre" });
  if (row.coveredBy) list.push({ text: `${row.coveredBy} news`, kind: "news" });
  if (row.links) list.push({ text: row.links === 1 ? "→ paper" : `→ ${row.links} papers`, kind: "link" });
  for (const s of row.interests.filter((s) => s.reached)) list.push({ text: s.interest, kind: "interest" });
  return list;
}

/** A likeness as the list shows it: a whole number, none before the library is indexed. */
export const likeText = (like: number | null) => (like === null ? "–" : Math.round(like).toString());

/** A score as the list shows it: one decimal, none without. */
export const scoreText = (score: number) => (score > 0 ? score.toFixed(1) : "–");

/** Why there is no score, with no interest to score in. */
export const NO_INTERESTS = "No interests yet: add them in Settings → Feeds";

/** What a score of the list says on hover: why there is none, no interest matched or none to match (`interests`
 *  off); a score says nothing. */
export const scoreHint = (score: number, interests: boolean) =>
  score > 0 ? undefined : interests ? "No interest matched" : NO_INTERESTS;

/** A count of citations as the list shows it: "1,234"; none when OpenAlex did not count them. */
export const citedText = (cited: number | null) => (cited === null ? "–" : cited.toLocaleString("en-US"));

/** The year of a date (ISO), as the list of the Search tab shows it. */
export const yearOf = (date: string | null) => date?.slice(0, 4) ?? "";
