// The items of the Feeds tab: their triage by the keys, the list kept in place while they are triaged, what an item
// adds to the library, how old things are said to be. Pure functions, tested by scripts/feeds.test.ts (npm test).

import { arxivDoi } from "../arxiv.ts";
import type { FeedDetail, FeedRow, FetchSummary, Linked, Triage } from "./api.ts";

/** the keys that change the triage of the items selected */
export type TriageAction = "shortlist" | "dismiss" | "read";

/**
 * The triage a key gives the items selected, and which of them it changes. Each key toggles: `s` shortlists, or
 * when all are shortlisted takes them back to seen; `x` dismisses, or brings back dismissed ones; `m` marks new
 * items seen, or when none is new marks the seen ones new again (a shortlisted, dismissed or added item keeps its
 * triage). None: nothing to change.
 */
export function toggled(items: { id: string; triage: Triage }[], action: TriageAction): { ids: string[]; triage: Triage } | null {
  const all = (t: Triage) => items.length > 0 && items.every((i) => i.triage === t);
  const change = (pick: (t: Triage) => boolean, triage: Triage) => {
    const ids = items.filter((i) => pick(i.triage) && i.triage !== triage).map((i) => i.id);
    return ids.length ? { ids, triage } : null;
  };
  switch (action) {
    case "shortlist":
      return all("shortlisted") ? change(() => true, "seen") : change(() => true, "shortlisted");
    case "dismiss":
      return all("dismissed") ? change(() => true, "seen") : change(() => true, "dismissed");
    case "read":
      return items.some((i) => i.triage === "new")
        ? change((t) => t === "new", "seen")
        : change((t) => t === "seen", "new");
  }
}

/** Mark all above as seen: the new items before `focus` in the list (all of them when it is not in the list). */
export function newAbove(rows: { id: string; triage: Triage }[], focus: string | null): string[] {
  const at = focus === null ? -1 : rows.findIndex((r) => r.id === focus);
  return rows.slice(0, at < 0 ? rows.length : at).filter((r) => r.triage === "new").map((r) => r.id);
}

/** The items from one to another, in the order of the list (one alone if either is not in it). */
export function rangeOf(ids: string[], from: string, to: string): string[] {
  const [a, b] = [ids.indexOf(from), ids.indexOf(to)].sort((x, y) => x - y);
  return a < 0 ? [to] : ids.slice(a, b + 1);
}

/** The item to show after some go from the list: the first after them that stays, else the last before. */
export function nextAfter(ids: string[], gone: Set<string>): string | null {
  const last = ids.reduce((at, id, i) => (gone.has(id) ? i : at), -1);
  return ids.slice(last + 1).find((id) => !gone.has(id)) ?? ids.slice(0, last).reverse().find((id) => !gone.has(id)) ?? null;
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

/** What the field of the Add sheet starts with, for a DOI or an arXiv id: the arXiv id as its DOI. */
export const addField = (id: string) => arxivDoi(id) ?? id.trim();

/** The link of a post most likely its paper: the first of high confidence, else the first. */
export const bestLink = (links: Linked[]): Linked | null => links.find((l) => l.confidence === "high") ?? links[0] ?? null;

/**
 * What adding an item adds: a paper's DOI (else its arXiv id), and the item that becomes *added*; a post's linked
 * paper, by its DOI, and that paper's item when it is one. None: nothing to add (a post without a link).
 */
export function addTarget(d: Pick<FeedDetail, "id" | "kind" | "doi" | "arxiv" | "links">): { id: string; item: string | null } | null {
  if (d.kind === "news" || d.kind === "blog") {
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
  const when = ago(last.at, now);
  const parts = [when === "now" ? "fetched just now" : /^\d+ (min|h|d)$/.test(when) ? `fetched ${when} ago` : `fetched on ${when}`];
  if (last.added) parts.push(`${last.added} new`);
  if (last.unchanged) parts.push(`${last.unchanged} not modified`);
  if (last.failed.length) parts.push(`${last.failed.length} failed`);
  return parts.join(" · ");
}

/** The badges of a row, after its title. */
export function badges(row: FeedRow): { text: string; kind: "lib" | "pre" | "news" | "link" | "interest" }[] {
  const list: ReturnType<typeof badges> = [];
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
