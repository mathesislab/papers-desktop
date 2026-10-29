// What can be done with the papers selected in the library table: which items of the context menu apply, the
// row to select after some are removed, and what is said of what was done. Pure functions on rows, tested by
// scripts/selection.test.ts (npm test).

import type { Opened, Row, TagCount, Transferred } from "../types.ts";
import { sameTag } from "../tags.ts";
import { fileName } from "./filters.ts";

/** above this many papers, opening them asks first */
export const OPEN_ASK = 5;

/** "1 paper", "3 papers"; `what` is the singular */
export const count = (n: number, what = "paper") => `${n} ${what}${n === 1 ? "" : "s"}`;

/** Which actions of the context menu apply to the papers: what does not is disabled. */
export type Available = {
  /** the PDF of at least one is there: Open in Preview, Reveal */
  pdf: boolean;
  /** at least one has a DOI */
  doi: boolean;
  /** one paper only */
  one: boolean;
  /** one paper, without a file: its PDF can be added */
  addPdf: boolean;
};

export function available(rows: Row[]): Available {
  return {
    pdf: rows.some((r) => r.file === "found"),
    doi: rows.some((r) => !!r.doi),
    one: rows.length === 1,
    addPdf: rows.length === 1 && rows[0].file === "none",
  };
}

/** The .bib files that papers can be copied or moved to: the others of the library, with several; not one that
 *  has all of them already. */
export function targetsOf(rows: Row[], bibs: string[]): string[] {
  if (bibs.length < 2) return [];
  return bibs.filter((bib) => !rows.every((r) => r.bib === bib));
}

/** The row to select once the papers `removed` are gone from the list `shown`: the first one after the first
 *  removed that stays, else the last one before; none when no row stays. */
export function nextAfter(shown: Row[], removed: Set<string>): string | null {
  const first = shown.findIndex((r) => removed.has(r.id));
  if (first < 0) return null;
  const stays = (r: Row) => !removed.has(r.id);
  return (shown.slice(first).find(stays) ?? shown.slice(0, first).reverse().find(stays))?.id ?? null;
}

/** What is said after papers are copied or moved to a .bib file. */
export function transferMessage(done: Transferred, bib: string, move: boolean): string {
  const to = fileName(bib);
  const parts = [
    done.added.length
      ? `${move ? "Moved" : "Copied"} ${count(done.added.length)} to ${to}`
      : `Nothing ${move ? "moved" : "copied"}`,
  ];
  if (done.existing.length) parts.push(`${done.existing.length} already in ${to}`);
  if (done.missing) parts.push(`${count(done.missing, "file")} not found`);
  return parts.join(", ");
}

/** What is said after PDFs are opened outside the app. */
export function openedMessage({ opened, skipped }: Opened): string {
  if (!opened) return skipped ? `No PDF to open (${count(skipped)} without one)` : "Nothing to open";
  return `Opened ${count(opened, "PDF")}` + (skipped ? `, ${count(skipped)} without a PDF skipped` : "");
}

/** What is said after papers are deleted. */
export function deletedMessage(n: number, files: boolean): string {
  return `Deleted ${count(n)}` + (files ? (n === 1 ? " and its PDF" : " and their PDFs") : "");
}

/** the tags in the context menu besides the ones pinned to the keys: the most used */
const COMMON_TAGS = 8;

/** The tags to put on papers or take off from the context menu: those pinned to the keys, then the most used. */
export function menuTags(pinned: (string | null)[], tags: TagCount[]): string[] {
  const list = pinned.filter((t): t is string => !!t);
  const common = tags
    .filter((t) => t.count > 0 && !list.some((p) => sameTag(p, t.tag)))
    .sort((a, b) => b.count - a.count || a.tag.localeCompare(b.tag))
    .slice(0, COMMON_TAGS);
  return [...list, ...common.map((t) => t.tag)];
}
