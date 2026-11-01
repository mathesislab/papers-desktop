// The commands of the Search tab (src-tauri/src/search.rs): a search of OpenAlex's works, page by page, the works
// found again, those starred, a search subscribed to in the Feeds tab, a search online (its address). The works
// found are items of a folder of feeds of their own: their details, triage, abstracts and PDFs are the Feeds tab's
// commands with the store "search" ($lib/feeds/api.ts).

import { invoke } from "@tauri-apps/api/core";
import type { FeedRow, SubscriptionInfo } from "$lib/feeds/api";

/** The order of the works found: the best match first (without words, the most cited), the most cited, the newest. */
export type Order = "relevance" | "cited" | "newest";

/** A page of a search. */
export interface SearchPage {
  rows: FeedRow[];
  /** the works found in all */
  total: number;
  /** the cursor of the next page; none after the last */
  next: string | null;
}

/** A page of the works of a query: the first (`cursor` none), whose works take the place of those found before but
 *  for those starred or added; or the next. Refused, said, for what OpenAlex cannot be asked. */
export const run = (query: string, order: Order, cursor: string | null) =>
  invoke<SearchPage>("search_run", { query, order, cursor });
/** The rows of works found again, in that order, the query's words marked: after a triage or an add. */
export const rowsOf = (ids: string[], query: string) => invoke<FeedRow[]>("search_rows", { ids, query });
/** The works starred, the latest found first. */
export const starred = () => invoke<FeedRow[]>("search_starred");
/** A query subscribed to in the Feeds tab, as a sweep of OpenAlex: the subscription made, to fetch. */
export const subscribe = (query: string) => invoke<SubscriptionInfo>("search_subscribe", { query });
/** The services of *Search online*, by name: Google Scholar, Google, then the user's of the settings. */
export const services = () => invoke<string[]>("search_services");
/** The address of a search of a service online, to open in the browser; refused, said, for what the service cannot
 *  be asked. */
export const online = (service: string, query: string) => invoke<string>("search_online", { service, query });

/** The events the page listens to. */
export const EVENTS = {
  /** the works found changed: a triage, an abstract looked up, the library indexed again */
  changed: "search-changed",
} as const;
