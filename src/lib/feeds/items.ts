// The items of the Feeds tab: their triage by the keys, the list kept in place while they are triaged, when an item
// shown counts as seen, what an item adds to the library, how old things are said to be. Pure, tested by
// scripts/feeds.test.ts (npm test).

import type { FeedDetail, FeedRow, FetchSummary, ItemKind, Linked, Rated, Reminder, Scored, Stages, Triage } from "./api.ts";
import { ITEM_KINDS, isPost } from "./kinds.ts";

/** the keys that change the triage of the items selected */
export type TriageAction = "star" | "remove" | "read";

/** Whether an item can be starred: one added to the library keeps being added. */
export const starrable = (item: { triage: Triage }) => item.triage !== "added";

/**
 * The triage a key gives the items selected, and which of them it changes. Each key toggles: `s` stars, or when
 * all are starred takes them back to seen (an added item keeps its triage); `x` removes, or brings back removed
 * ones; `m` marks new items seen, or when none is new marks the seen ones new again (a starred, removed or added
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
    case "remove":
      return all("removed") ? change(() => true, "seen") : change(() => true, "removed");
    case "read":
      return items.some((i) => i.triage === "new")
        ? change((t) => t === "new", "seen")
        : change((t) => t === "seen", "new");
  }
}

/** A row unread: new, not snoozed. */
export const unread = (r: { triage: Triage; reminder?: Reminder | null }) => r.triage === "new" && r.reminder !== "snoozed";

/** Mark all above as seen: the unread items before `focus` in the list (all of them when it is not in the list). */
export function newAbove(rows: { id: string; triage: Triage; reminder?: Reminder | null }[], focus: string | null): string[] {
  const at = focus === null ? -1 : rows.findIndex((r) => r.id === focus);
  return rows.slice(0, at < 0 ? rows.length : at).filter(unread).map((r) => r.id);
}

/** Mark as read, older than: the unread items dated more than some days before now (one without a date is not). */
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

/** What the status of an item says (`marks`): a row's, or the details' (`detailMarked`). */
export interface Markable {
  kind: ItemKind;
  inLibrary: string[];
  stages: Stages;
  /** the posts that link to it */
  coveredBy: number;
  /** a post: the papers found behind it */
  links: number;
}

/** A symbol of the status of an item, in its slot (the slots in this order: its version or, a post, its kind; the
 *  library; the news), with what it says in words (on hover; in the details beside it). An item in
 *  the library has its entries, by the .bib of each when the library has several (else null), once a .bib. */
export type Mark =
  | { slot: "version"; symbol: "preprint" | "published" | "both" | "news" | "blog"; text: string }
  | { slot: "library"; symbol: "library"; text: string; entries: { id: string; bib: string | null }[] }
  | { slot: "news"; symbol: "covered"; text: string };

export const MARK_SLOTS = ["version", "library", "news"] as const;

/** The status of an item as symbols: a paper a preprint (◐), published (◑) or both known (●), a post its kind with
 *  the papers behind it; in the library, by its .bib files (`libraryOf`, "in thesis.bib"; none: "in library"); the
 *  news that covered it. */
export function marks(item: Markable, libraryOf: (id: string) => string | null): Mark[] {
  const list: Mark[] = [];
  if (isPost(item.kind)) {
    const behind = item.links ? ` · → ${item.links === 1 ? "1 paper" : `${item.links} papers`}` : "";
    list.push({ slot: "version", symbol: item.kind === "news" ? "news" : "blog", text: ITEM_KINDS[item.kind].name + behind });
  } else {
    list.push({ slot: "version", symbol: item.stages, text: item.stages === "both" ? "preprint and published" : item.stages });
  }
  if (item.inLibrary.length) {
    const entries = new Map<string | null, string>();
    for (const id of item.inLibrary) {
      const bib = libraryOf(id);
      if (!entries.has(bib)) entries.set(bib, id);
    }
    const bibs = [...entries.keys()];
    const text = bibs.every((b) => b === null) ? "in library" : `in ${bibs.filter((b) => b !== null).join(", ")}`;
    list.push({ slot: "library", symbol: "library", text, entries: [...entries].map(([bib, id]) => ({ id, bib })) });
  }
  if (item.coveredBy) list.push({ slot: "news", symbol: "covered", text: `${item.coveredBy} news` });
  return list;
}

/** The reminder of a row, by its date (⏰): due, or snoozed till a day ("reminder 3 Nov"); none without one. */
export function reminderOf(row: { reminder: Reminder | null; remind: string | null }): { due: boolean; text: string } | null {
  if (row.reminder === "due") return { due: true, text: "Reminder due" };
  if (row.reminder !== "snoozed" || !row.remind) return null;
  const day = new Date(row.remind).toLocaleDateString(undefined, { day: "numeric", month: "short" });
  return { due: false, text: `Reminder ${day}` };
}

/** The stages of a paper known: its own (a preprint's, else published) and those of its versions (as
 *  `Item::stages` in Rust). */
export function stagesOf(kind: ItemKind, versions: ("preprint" | "published")[]): Stages {
  const known = (stage: "preprint" | "published") => (kind === "preprint") === (stage === "preprint") || versions.includes(stage);
  return known("preprint") ? (known("published") ? "both" : "preprint") : "published";
}

/** The details of an item as its status sees them. */
export const detailMarked = (d: FeedDetail): Markable => ({
  kind: d.kind,
  inLibrary: d.inLibrary,
  stages: stagesOf(d.kind, d.versions.map((v) => v.stage)),
  coveredBy: d.coveredBy.length,
  links: d.links.length,
});

/** Why a score has no keywords: no interest to score in. */
export const NO_INTERESTS = "No interests yet: add them in Settings → General → Score";

/** What the score of a row says on hover: "Score 72 · keywords 72 (ice sheet, AMOC) · library 41", the terms its
 *  interests matched, the best interest's first; vetoed, the exclude that did ("Excluded: Georgia"); without
 *  interests, why the keywords are missing, on a line of its own. A library not indexed yet says no library. */
export function scoreHint(row: Rated & { interests: Scored[] }): string {
  if (row.excluded !== null) return `Excluded: ${row.excluded}`;
  const terms = [...new Set(row.interests.flatMap((s) => s.terms))];
  const parts = [`Score ${Math.round(row.score)}`];
  if (row.keywords !== null) parts.push(`keywords ${Math.round(row.keywords)}` + (terms.length ? ` (${terms.join(", ")})` : ""));
  if (row.library !== null) parts.push(`library ${Math.round(row.library)}`);
  return [parts.join(" · "), ...(row.keywords === null ? [NO_INTERESTS] : [])].join("\n");
}

/** How much of its track a part of the score fills, in percent: a part missing none. */
export const barOf = (part: number | null) => Math.max(0, Math.min(100, part ?? 0));

/** A count of citations as the list shows it: "1,234"; none when OpenAlex did not count them. */
export const citedText = (cited: number | null) => (cited === null ? "–" : cited.toLocaleString("en-US"));

/** The year of a date (ISO), as the list of the Search tab shows it. */
export const yearOf = (date: string | null) => date?.slice(0, 4) ?? "";
