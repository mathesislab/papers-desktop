<script lang="ts">
  // The details of the item selected in the Feeds tab or the Search tab, to read: the buttons of the tab (they do
  // what the keys do), its title, authors and where it appeared, then a paper's abstract, marked where the query or
  // an interest matched, and what tells whether to add it (Facts); a post's text, and the papers behind it, each to
  // add, star or show (*Find linked paper* looks for them again).

  import type { Snippet } from "svelte";
  import type { FeedDetail, FeedRow, Linked, Store } from "./api";
  import Facts, { inLibrary } from "./Facts.svelte";
  import { dayText } from "./items";
  import PostHtml from "./PostHtml.svelte";
  import { ITEM_KINDS, isPost } from "./kinds";
  import Spans from "./Spans.svelte";

  let {
    detail,
    wide = false,
    count,
    byId,
    colours,
    store = "feeds",
    actions,
    finding = false,
    onaddlink,
    onstarlink,
    onfind,
    onitem,
    onentry,
    onback,
  }: {
    detail: FeedDetail;
    /** read in place of the list: the texts a size up */
    wide?: boolean;
    /** how many items are selected: the buttons act on all of them */
    count: number;
    /** the rows of the list, by id: a linked paper that is an item of the feeds */
    byId: Map<string, FeedRow>;
    colours: Map<string, string>;
    /** the folder of feeds of the item: its free PDF is asked there */
    store?: Store;
    /** the buttons of the tab, at the top, for the item shown */
    actions: Snippet<[FeedDetail]>;
    /** a post's papers (the Feeds tab's: the works found in the Search tab are no posts): being looked for; one
     *  added, by its DOI; one starred, or no longer; looked for again */
    finding?: boolean;
    onaddlink?: (link: Linked) => void;
    onstarlink?: (item: string) => void;
    onfind?: () => void;
    onitem: (id: string) => void;
    onentry: (id: string) => void;
    /** read in place of the list: back to it, as Esc */
    onback?: () => void;
  } = $props();

  let post = $derived(isPost(detail.kind));
  let date = $derived(detail.date ? dayText(detail.date) : null);
  let cited = $derived(detail.cited === null ? null : `cited ${detail.cited.toLocaleString("en-US")}×`);
  let where = $derived([detail.journal, date, cited, post ? null : detail.doi].filter((x): x is string => !!x).join(" · "));
  const VIA: Record<Linked["via"], string> = { feed: "named in the feed", page: "found on the page", title: "found by its title" };
</script>

<div class="details" class:wide>
  {#if count > 1}
    <p class="selection">{count} items selected: the buttons and keys act on all of them</p>
  {/if}
  <!-- the tab's own buttons: styled here (`.actions :global(button)`), as those of a linked paper -->
  <div class="actions">
    {@render actions(detail)}
  </div>

  <div>
    <h3><Spans spans={detail.title} /></h3>
    {#if detail.authors.length}<p class="authors">{detail.authors.join(", ")}</p>{/if}
    <p class="by">
      {#if detail.kind !== "paper"}<span class="badge {detail.kind}">{ITEM_KINDS[detail.kind].name}</span>{/if}
      {#if detail.editorial}<span class="badge">editorial</span>{/if}
      {where}
    </p>
    {#if detail.inLibrary.length}
      <p class="by">{@render inLibrary(detail.inLibrary, onentry)}</p>
    {/if}
  </div>

  {#if post}
    <section>
      <h5>Post</h5>
      {#if detail.content}
        <div class="text"><PostHtml html={detail.content} /></div>
      {:else if detail.abstract.length}
        <p class="text"><Spans spans={detail.abstract} /></p>
      {:else}
        <p class="muted">No text in the feed</p>
      {/if}
    </section>
    <section>
      <h5>{detail.links.length === 1 ? "Linked paper" : detail.links.length ? `Linked papers (${detail.links.length})` : "Linked paper"}</h5>
      {#each detail.links as link (link.doi)}
        {@const row = link.item ? byId.get(link.item) : undefined}
        <div class="card">
          <div class="title">{link.title ?? link.doi}</div>
          <div class="muted">
            {link.doi} · {VIA[link.via]}{#if link.confidence === "low"} · <span class="badge">maybe</span>{/if}
          </div>
          {#if row}
            <div><span class="badge">also in your feed{row.score ? ` · score ${row.score.toFixed(1)}` : ""}</span></div>
          {/if}
          <div class="actions">
            <button class="primary" onclick={() => onaddlink?.(link)}>Add</button>
            {#if link.item}
              <button onclick={() => onstarlink?.(link.item!)}>{row?.triage === "starred" ? "★ Starred" : "☆ Star"}</button>
              <button onclick={() => onitem(link.item!)}>Show paper</button>
            {/if}
          </div>
        </div>
      {:else}
        <p class="muted">None found yet</p>
      {/each}
      <div class="actions">
        <button onclick={onfind} disabled={finding}>{finding ? "Looking…" : "Find linked paper"} <kbd>F</kbd></button>
      </div>
    </section>
  {:else}
    <section class="abstract">
      <h5>{detail.abstractFromFeed ? "Summary from the feed" : "Abstract"}</h5>
      {#if detail.abstract.length}
        <p class="text"><Spans spans={detail.abstract} /></p>
      {:else}
        <p class="muted">No abstract</p>
      {/if}
    </section>
  {/if}

  <Facts {detail} {colours} {store} {onitem} {onentry} />

  {#if onback}
    <button class="back" onclick={onback}>Back to the list <kbd>Esc</kbd></button>
  {/if}
</div>

<style>
  .details {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 11px;
    padding: 14px 16px;
    -webkit-user-select: text;
    user-select: text;
  }
  .wide {
    padding: 18px 24px 28px;
    gap: 14px;
  }
  .wide h3 {
    font-size: 20px;
  }
  .wide .authors {
    font-size: 14px;
  }
  .wide .abstract .text {
    font-size: 15px;
    line-height: 1.6;
  }
  .selection {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
  }
  h3 {
    margin: 0 0 5px;
    font-size: 16px;
    line-height: 1.3;
  }
  .authors {
    margin: 0 0 3px;
    line-height: 1.4;
  }
  .by {
    margin: 0 0 3px;
    color: var(--muted);
    line-height: 1.4;
  }
  section {
    padding-top: 9px;
    border-top: 1px solid var(--line);
  }
  h5 {
    margin: 0 0 6px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }
  p {
    margin: 0 0 3px;
  }
  .text {
    margin: 0 0 8px;
    line-height: 1.5;
  }
  /* what is read: no rule above it, a size up */
  .abstract {
    padding-top: 0;
    border-top: none;
  }
  .abstract .text {
    font-size: 13.5px;
    line-height: 1.55;
  }
  .muted {
    color: var(--muted);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  section .actions {
    margin-top: 6px;
  }
  button,
  .actions :global(button) {
    padding: 3px 9px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
    white-space: nowrap;
  }
  button:disabled,
  .actions :global(button:disabled) {
    opacity: 0.4;
  }
  button.primary,
  .actions :global(button.primary) {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
  /* lightly in the feeds' green, as the library's light blue buttons: the paper's page, to decide */
  .actions :global(button.open) {
    border-color: color-mix(in srgb, var(--feeds) 40%, transparent);
    background: color-mix(in srgb, var(--feeds) 14%, transparent);
    color: var(--feeds);
    font-weight: 500;
  }
  .actions :global(button.open:hover:not(:disabled)) {
    background: color-mix(in srgb, var(--feeds) 24%, transparent);
  }
  /* at the end of the item read: the list again */
  button.back {
    flex: none;
    align-self: center;
    width: min(100%, 420px);
    margin-top: 8px;
    padding: 7px 12px;
  }
  button.back:hover {
    background: var(--line);
  }
  kbd,
  .actions :global(kbd) {
    margin-left: 3px;
    font: 10px ui-monospace, monospace;
    opacity: 0.7;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 5px;
    margin-bottom: 6px;
    padding: 9px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .card .title {
    font-weight: 600;
    line-height: 1.3;
  }
  .card .actions {
    margin-top: 2px;
  }
  .badge {
    padding: 0 5px;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--muted);
    font-size: 10.5px;
  }
  .badge.preprint {
    border-color: #a16207;
    color: #a16207;
  }
  .badge.news {
    border-color: #c2410c;
    color: #c2410c;
  }
  .badge.blog {
    border-color: #0e7490;
    color: #0e7490;
  }
</style>
