<script lang="ts">
  // The details of the item selected in the Feeds tab. A paper: its abstract, marked where the query or an
  // interest matched, and what tells whether to add it (Facts). A post: its text, and the papers behind it, each
  // to add, shortlist or show; *Find linked paper* looks for them again. The buttons do what the keys do.

  import type { FeedDetail, FeedRow, Linked, PdfStatus } from "./api";
  import Facts from "./Facts.svelte";
  import { isPost } from "./filters";
  import type { TriageAction } from "./items";
  import Spans from "./Spans.svelte";

  let {
    detail,
    count,
    byId,
    colours,
    finding,
    pdfOf,
    onadd,
    onaddlink,
    ontriage,
    onshortlistlink,
    onopen,
    onfind,
    onitem,
    onentry,
  }: {
    detail: FeedDetail;
    /** how many items are selected: the buttons act on all of them */
    count: number;
    /** the rows of the list, by id: a linked paper that is an item of the feeds */
    byId: Map<string, FeedRow>;
    colours: Map<string, string>;
    /** the papers of the post are being looked for */
    finding: boolean;
    pdfOf: (id: string) => Promise<PdfStatus>;
    /** the item added: the sheet, with or without the free PDF */
    onadd: (pdf: boolean) => void;
    /** a paper of the post added, by its DOI */
    onaddlink: (link: Linked) => void;
    ontriage: (action: TriageAction) => void;
    /** a paper of the post shortlisted, or no longer */
    onshortlistlink: (item: string) => void;
    onopen: () => void;
    onfind: () => void;
    onitem: (id: string) => void;
    onentry: (id: string) => void;
  } = $props();

  let post = $derived(isPost(detail.kind));
  let date = $derived(
    detail.date ? new Date(detail.date).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" }) : null,
  );
  let by = $derived(
    [detail.authors.join(", "), detail.journal, date, post ? null : detail.doi].filter((x): x is string => !!x).join(" · "),
  );
  const KIND: Record<FeedDetail["kind"], string> = { paper: "paper", preprint: "preprint", news: "news & press", blog: "blog" };
  const VIA: Record<Linked["via"], string> = { feed: "named in the feed", page: "found on the page", search: "found by its title" };
</script>

<div class="details">
  {#if count > 1}
    <p class="selection">{count} items selected: the buttons and keys act on all of them</p>
  {/if}
  <div>
    <h3><Spans spans={detail.title} /></h3>
    <p class="by">
      {#if detail.kind !== "paper"}<span class="badge {detail.kind}">{KIND[detail.kind]}</span>{/if}
      {#if detail.editorial}<span class="badge">editorial</span>{/if}
      {by}
    </p>
    {#if detail.inLibrary.length}
      <p class="by">
        <span class="badge lib">in library</span>
        {#each detail.inLibrary as id (id)}<button class="link" onclick={() => onentry(id)}>show</button>{/each}
      </p>
    {/if}
  </div>

  <div class="actions">
    {#if post}
      <button class="primary" onclick={onopen} disabled={!detail.url}>Open post ↗ <kbd>O</kbd></button>
      <button onclick={() => ontriage("read")}>{detail.triage === "new" ? "Mark read" : "Mark unread"} <kbd>M</kbd></button>
      <button onclick={() => ontriage("shortlist")}>{detail.triage === "shortlisted" ? "★ Shortlisted" : "★ Shortlist"} <kbd>S</kbd></button>
      <button onclick={() => ontriage("dismiss")}>{detail.triage === "dismissed" ? "Dismissed" : "Dismiss"} <kbd>X</kbd></button>
    {:else}
      <button class="primary" onclick={() => onadd(true)} title="Add to the library, with the free PDF if there is one">Add <kbd>A</kbd></button>
      <button onclick={() => onadd(false)} title="Add to the library without looking for its PDF">Add without PDF</button>
      <button onclick={() => ontriage("shortlist")}>{detail.triage === "shortlisted" ? "★ Shortlisted" : "★ Shortlist"} <kbd>S</kbd></button>
      <button onclick={onopen} disabled={!detail.url && !detail.doi}>Open ↗ <kbd>O</kbd></button>
      <button onclick={() => ontriage("dismiss")}>{detail.triage === "dismissed" ? "Dismissed" : "Dismiss"} <kbd>X</kbd></button>
    {/if}
  </div>

  {#if post}
    <section>
      <h5>{detail.content.length ? "Post" : "Post · the teaser of the feed"}</h5>
      {#if detail.content.length}
        {#each detail.content as paragraph}<p class="text">{paragraph}</p>{/each}
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
            <button class="primary" onclick={() => onaddlink(link)}>Add</button>
            {#if link.item}
              <button onclick={() => onshortlistlink(link.item!)}>{row?.triage === "shortlisted" ? "★ Shortlisted" : "★ Shortlist"}</button>
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
    <section>
      <h5>Abstract{detail.abstractLookedUp ? " · from Crossref or OpenAlex (the feed had none, or a teaser)" : ""}</h5>
      {#if detail.abstract.length}
        <p class="text"><Spans spans={detail.abstract} /></p>
      {:else}
        <p class="muted">No abstract</p>
      {/if}
    </section>
  {/if}

  <Facts {detail} {colours} {pdfOf} {onitem} {onentry} />
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
  .selection {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
  }
  h3 {
    margin: 0 0 4px;
    font-size: 15px;
    line-height: 1.3;
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
  button {
    padding: 3px 9px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
    white-space: nowrap;
  }
  button:disabled {
    opacity: 0.4;
  }
  button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
  button.link {
    padding: 0;
    border: none;
    color: var(--accent);
  }
  kbd {
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
  .badge.lib {
    border-color: #2f9e6e;
    color: #2f9e6e;
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
