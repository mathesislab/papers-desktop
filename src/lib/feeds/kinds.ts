// The kinds of the items of the feeds and of their subscriptions, each named once: one item or subscription
// (`name`, sentence case; a menu titles it), several (`plural`: the sidebar's Kinds and Sources), an item's mark (the
// sidebar; the list leaves a paper unmarked). Pure, tested by scripts/feeds.test.ts (npm test).

import type { ItemKind, SourceKind } from "./api.ts";

export interface Kind {
  name: string;
  plural: string;
  /** what tells it, in the list and the sidebar */
  mark: string;
}

export const ITEM_KINDS: Record<ItemKind, Kind> = {
  paper: { name: "paper", plural: "Papers", mark: "▤" },
  preprint: { name: "preprint", plural: "Preprints", mark: "◐" },
  news: { name: "news & press", plural: "News & press", mark: "✦" },
  blog: { name: "general", plural: "General", mark: "✎" },
};

/** the kinds of a subscription, which its items take; what one is for (`hint`), where it is chosen */
export const SOURCE_KINDS: Record<SourceKind, Omit<Kind, "mark"> & { hint?: string }> = {
  journal: { name: "Journal", plural: "Journals" },
  preprints: { name: "Preprint server", plural: "Preprint servers" },
  news: { name: "News & press", plural: "News & press" },
  blog: { name: "General", plural: "General", hint: "blogs, comics, …" },
};

/** The kinds in their order. */
export const itemKinds = Object.keys(ITEM_KINDS) as ItemKind[];
export const sourceKinds = Object.keys(SOURCE_KINDS) as SourceKind[];

export const isItemKind = (v: unknown): v is ItemKind => typeof v === "string" && Object.hasOwn(ITEM_KINDS, v);

/** A post: news or a blog; the rest are papers. */
export const isPost = (kind: ItemKind) => kind === "news" || kind === "blog";
