<script lang="ts">
  // Review of the shortlist (the mockup's view 3): over the list and the details, its items one at a time, best
  // score first, as the shortlist was when the review began; for each, what tells whether to add it (Facts). Keys:
  // a adds (the Add sheet, then the next), b adds without its PDF, o opens its page, l leaves it for later (still
  // shortlisted), x dismisses it, j and k go to the next and the one before, Esc leaves the review.

  import { onMount } from "svelte";
  import * as api from "./api";
  import type { FeedDetail, PdfStatus, Triage } from "./api";
  import Facts from "./Facts.svelte";
  import { isPost } from "./filters";
  import { addTarget } from "./items";
  import { REVIEW_REQUEST, decide, progressOf, reviewed, startReview, stepReview, summaryOf, type Review } from "./review";
  import Spans from "./Spans.svelte";

  let {
    colours,
    pdfOf,
    onadd,
    ontriage,
    onopen,
    onitem,
    onentry,
    onclose,
  }: {
    colours: Map<string, string>;
    pdfOf: (id: string) => Promise<PdfStatus>;
    /** the item's paper added, by the Add sheet; whether it was */
    onadd: (target: { id: string; item: string | null }, pdf: boolean) => Promise<boolean>;
    ontriage: (id: string, triage: Triage) => Promise<void>;
    /** the page of the item in the browser */
    onopen: (id: string) => void;
    /** an item of the feeds to show: the review ends */
    onitem: (id: string) => void;
    onentry: (id: string) => void;
    onclose: () => void;
  } = $props();

  let review = $state<Review>(startReview([]));
  let loaded = $state(false);
  let detail = $state<FeedDetail | null>(null);
  let message = $state<string | null>(null);

  let id = $derived(reviewed(review));
  let progress = $derived(progressOf(review));
  let by = $derived.by(() => {
    if (!detail) return "";
    const date = detail.date
      ? new Date(detail.date).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" })
      : null;
    return [detail.authors.join(", "), detail.journal, date, detail.doi].filter((x): x is string => !!x).join(" · ");
  });

  onMount(async () => {
    try {
      const answer = await api.view(REVIEW_REQUEST);
      review = startReview(answer.rows.map((r) => r.id));
    } catch (e) {
      message = String(e);
    }
    loaded = true;
  });

  // the item shown, its abstract marked by the interests
  $effect(() => {
    const shown = id;
    message = null;
    if (!shown) {
      detail = null;
      return;
    }
    let live = true;
    api
      .detail(shown, "")
      .then((d) => live && (detail = d))
      .catch((e) => live && ((detail = null), (message = String(e))));
    return () => {
      live = false;
    };
  });

  /** a, b: the paper added, then the next; a sheet closed stays on it. */
  async function add(pdf: boolean) {
    if (!detail || detail.id !== id) return;
    const target = addTarget(detail);
    if (!target) {
      message = isPost(detail.kind) ? "No paper linked to the post: L leaves it for later" : "No DOI or arXiv id to add it by";
      return;
    }
    if (await onadd(target, pdf)) review = decide(review, "added");
  }

  async function dismiss() {
    if (!id) return;
    await ontriage(id, "dismissed");
    review = decide(review, "dismissed");
  }

  /** The keys of the review, from the window's. */
  export function key(e: KeyboardEvent) {
    if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
    const act = (action: () => unknown) => {
      e.preventDefault();
      action();
    };
    switch (e.key) {
      case "Escape":
        return act(onclose);
      case "j":
      case "ArrowDown":
      case "ArrowRight":
        return act(() => (review = stepReview(review, 1)));
      case "k":
      case "ArrowUp":
      case "ArrowLeft":
        return act(() => (review = stepReview(review, -1)));
      case "a":
        return act(() => add(true));
      case "b":
        return act(() => add(false));
      case "o":
        return act(() => id && onopen(id));
      case "l":
        return act(() => (review = decide(review, "later")));
      case "x":
        return act(dismiss);
    }
  }
</script>

<section class="review">
  <div class="head">
    <span>Shortlist · {progress.text}</span>
    <div class="progress" role="progressbar" aria-valuenow={Math.round(progress.fraction * 100)} aria-valuemin={0} aria-valuemax={100}>
      <i style:width="{progress.fraction * 100}%"></i>
    </div>
    <button class="link" onclick={onclose}>Esc leaves the review</button>
  </div>

  {#if !loaded}
    <p class="empty">Loading the shortlist…</p>
  {:else if !review.ids.length}
    <p class="empty">Nothing shortlisted: <kbd>S</kbd> in the list shortlists an item.</p>
  {:else if id === null}
    <div class="empty">
      <p>The shortlist is reviewed: {summaryOf(review)}.</p>
      <p><button onclick={() => (review = stepReview(review, -review.ids.length))}>Again from the first</button> <button class="primary" onclick={onclose}>Leave the review</button></p>
    </div>
  {:else if detail && detail.id === id}
    <div class="card">
      <div class="main">
        <h2><Spans spans={detail.title} /></h2>
        <p class="by">{by}</p>
        {#if review.done[detail.id]}
          <p class="done">{review.done[detail.id] === "later" ? "Left for later" : review.done[detail.id] === "added" ? "Added" : "Dismissed"}</p>
        {/if}
        {#if detail.abstract.length}
          <p class="abstract"><Spans spans={detail.abstract} /></p>
        {:else if detail.content.length}
          {#each detail.content as paragraph}<p class="abstract">{paragraph}</p>{/each}
        {:else}
          <p class="muted">No abstract</p>
        {/if}
        {#if isPost(detail.kind) && detail.links.length}
          <h5>Linked {detail.links.length === 1 ? "paper" : "papers"}</h5>
          {#each detail.links as link (link.doi)}
            <p>{link.title ?? link.doi} <span class="muted">{link.doi}</span></p>
          {/each}
        {/if}
      </div>
      <div class="facts">
        {#if detail.inLibrary.length}
          <p><span class="badge lib">in library</span>
            {#each detail.inLibrary as entry (entry)}<button class="link" onclick={() => onentry(entry)}>show</button>{/each}
          </p>
        {/if}
        <Facts {detail} {colours} {pdfOf} {onitem} {onentry} />
      </div>
    </div>
  {:else}
    <p class="empty">{message ?? "Loading…"}</p>
  {/if}
  {#if message && detail}
    <p class="message">{message}</p>
  {/if}

  {#if id !== null}
    <div class="keys">
      <button class="primary" onclick={() => add(true)}>Add with PDF <kbd>A</kbd></button>
      <button onclick={() => add(false)}>Add without PDF <kbd>B</kbd></button>
      <button onclick={() => id && onopen(id)}>Open article ↗ <kbd>O</kbd></button>
      <button onclick={() => (review = decide(review, "later"))}>Later <kbd>L</kbd></button>
      <button onclick={dismiss}>Dismiss <kbd>X</kbd></button>
      <span class="spacer"></span>
      <button disabled={review.at === 0} onclick={() => (review = stepReview(review, -1))}>← Previous <kbd>K</kbd></button>
      <button onclick={() => (review = stepReview(review, 1))}>Next → <kbd>J</kbd></button>
    </div>
  {/if}
</section>

<style>
  .review {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 18px;
    overflow-y: auto;
    padding: 18px 28px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--muted);
  }
  .progress {
    flex: 1;
    height: 5px;
    overflow: hidden;
    border-radius: 3px;
    background: var(--line);
  }
  .progress i {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.15s;
  }
  .card {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 300px;
    gap: 22px;
    -webkit-user-select: text;
    user-select: text;
  }
  h2 {
    margin: 0 0 6px;
    font-size: 19px;
    line-height: 1.3;
  }
  h5 {
    margin: 12px 0 6px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }
  .by {
    margin: 0 0 12px;
    color: var(--muted);
    line-height: 1.4;
  }
  .abstract {
    margin: 0 0 10px;
    font-size: 14px;
    line-height: 1.6;
  }
  .done {
    margin: 0 0 10px;
    color: var(--accent);
    font-size: 12px;
  }
  .facts {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .facts p {
    margin: 0;
  }
  .muted {
    color: var(--muted);
  }
  .message {
    margin: 0;
    color: var(--error);
  }
  .keys {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding-top: 12px;
    border-top: 1px solid var(--line);
  }
  .spacer {
    flex: 1;
  }
  button {
    padding: 5px 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
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
    font-size: 12px;
  }
  kbd {
    margin-left: 3px;
    font: 10px ui-monospace, monospace;
    opacity: 0.7;
  }
  .badge {
    padding: 0 5px;
    border: 1px solid var(--border);
    border-radius: 4px;
    font-size: 10.5px;
  }
  .badge.lib {
    border-color: #2f9e6e;
    color: #2f9e6e;
  }
  .empty {
    margin: auto;
    padding: 24px;
    color: var(--muted);
    text-align: center;
  }
</style>
