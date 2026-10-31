<script lang="ts">
  // What tells whether a paper of the feeds is worth adding, in the details and in the Review: why it is here (the
  // interests and the terms that matched), its free PDF (looked up a moment after it is shown, once per item), its
  // versions, the posts that cover it, where it was seen.

  import type { FeedDetail, PdfStatus } from "./api";

  let {
    detail,
    colours,
    pdfOf,
    onitem,
    onentry,
  }: {
    detail: FeedDetail;
    /** the colours of the interests */
    colours: Map<string, string>;
    /** the free PDF of an item, as OpenAlex knows it */
    pdfOf: (id: string) => Promise<PdfStatus>;
    /** an item of the feeds to show: a post that covers it, another version */
    onitem: (id: string) => void;
    /** a paper of the library to show */
    onentry: (id: string) => void;
  } = $props();

  /** how long an item is shown before its PDF is looked up: j held down does not ask for each */
  const PDF_DELAY = 300;

  let paper = $derived(detail.kind === "paper" || detail.kind === "preprint");
  let pdf = $state<PdfStatus | "looking" | { error: string } | null>(null);
  let scored = $derived([...detail.interests].filter((s) => s.score > 0).sort((a, b) => b.score - a.score));

  $effect(() => {
    const { id } = detail;
    const ask = paper && detail.doi !== null;
    pdf = ask ? "looking" : null;
    if (!ask) return;
    let live = true;
    const timer = setTimeout(() => {
      pdfOf(id)
        .then((p) => live && (pdf = p))
        .catch((e) => live && (pdf = { error: String(e) }));
    }, PDF_DELAY);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  });

  const ACCESS: Record<PdfStatus["access"], { text: string; tone: string }> = {
    open: { text: "Open access", tone: "ok" },
    preprint: { text: "Free preprint", tone: "warn" },
    closed: { text: "Publisher only", tone: "closed" },
    unknown: { text: "Not known", tone: "closed" },
  };
</script>

{#if scored.length}
  <section>
    <h5>Why it is here</h5>
    {#each scored as s (s.interest)}
      <p class:muted={!s.reached}>
        <span class="dot" style:background={colours.get(s.interest) ?? "var(--border)"}></span>
        {s.interest} {s.score.toFixed(1)}{#if s.terms.length}: {s.terms.join(", ")}{/if}{#if !s.reached} (below its minimum){/if}
      </p>
    {/each}
  </section>
{/if}

{#if paper && detail.doi}
  <section>
    <h5>PDF</h5>
    {#if pdf === "looking"}
      <p class="muted">Looking…</p>
    {:else if pdf && "error" in pdf}
      <p class="muted" title={pdf.error}>Not known</p>
    {:else if pdf}
      <p>
        <span class={ACCESS[pdf.access].tone}>● {ACCESS[pdf.access].text}</span>
        {#if pdf.licence}· {pdf.licence}{/if}
        {#if pdf.host}· {pdf.host}{/if}
      </p>
    {/if}
  </section>
{/if}

{#if detail.versions.length || detail.versionInLibrary.length}
  <section>
    <h5>Versions</h5>
    {#each detail.versions as v (v.doi)}
      <p>
        {v.stage === "preprint" ? "Preprint" : "Published"} <span class="muted">{v.doi}</span>
        {#if v.item && v.item !== detail.id}<button class="link" onclick={() => onitem(v.item!)}>show</button>{/if}
      </p>
    {/each}
    {#if detail.versionInLibrary.length}
      <p>
        <span class="badge lib">{detail.kind === "preprint" ? "published version in library" : "preprint in library"}</span>
        {#if detail.kind !== "preprint"}→ Add would replace it with this version.{/if}
        {#each detail.versionInLibrary as id (id)}
          <button class="link" onclick={() => onentry(id)}>show</button>
        {/each}
      </p>
    {/if}
  </section>
{/if}

{#if detail.coveredBy.length}
  <section>
    <h5>Covered by</h5>
    {#each detail.coveredBy as post (post.id)}
      <p><button class="link plain" onclick={() => onitem(post.id)}>✦ {post.source} · {post.title}</button></p>
    {/each}
  </section>
{/if}

{#if detail.sources.length}
  <section>
    <h5>Seen in</h5>
    <p>{detail.sources.map((s) => s.title).join(" · ")}</p>
  </section>
{/if}

<style>
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
    line-height: 1.45;
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--muted);
  }
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    margin-right: 2px;
    border-radius: 50%;
  }
  .ok {
    color: #2f9e6e;
  }
  .warn {
    color: #a16207;
  }
  .closed {
    color: var(--muted);
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
  button.link {
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }
  button.link.plain {
    color: var(--fg);
    text-align: left;
  }
  button.link.plain:hover {
    color: var(--accent);
  }
</style>
