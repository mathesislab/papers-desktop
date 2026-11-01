<script lang="ts" module>
  export { inLibrary };
</script>

<script lang="ts">
  // What tells whether a paper of the feeds is worth adding, in the details: why it is here (its score and its two
  // parts: its keywords, with the interests and the terms that matched, and its library; or the exclude that vetoed
  // it), its free PDF (looked up a moment after it is shown, once per item), its versions, the posts
  // that cover it, where it was seen, and last, set apart and quieter than what is read, the papers of the library
  // similar to it (each shown in the Library tab on a click).

  import { pdfOf, type FeedDetail, type PdfStatus, type Store } from "./api";
  import { NO_INTERESTS } from "./items";

  let {
    detail,
    colours,
    store = "feeds",
    onitem,
    onentry,
  }: {
    detail: FeedDetail;
    /** the colours of the interests */
    colours: Map<string, string>;
    /** the folder of feeds of the item: its free PDF is asked there */
    store?: Store;
    /** an item of the feeds to show: a post that covers it, another version */
    onitem: (id: string) => void;
    /** a paper of the library to show */
    onentry: (id: string) => void;
  } = $props();

  /** how long an item is shown before its PDF is looked up: j held down does not ask for each */
  const PDF_DELAY = 300;

  let paper = $derived(detail.kind === "paper" || detail.kind === "preprint");
  let pdf = $state<PdfStatus | "looking" | { error: string } | null>(null);
  /** a part of the score as said: a whole number, a dash when missing */
  const part = (n: number | null) => (n === null ? "–" : Math.round(n).toString());

  $effect(() => {
    const { id } = detail;
    const ask = paper && detail.doi !== null;
    pdf = ask ? "looking" : null;
    if (!ask) return;
    let live = true;
    const timer = setTimeout(() => {
      pdfOf(id, store)
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

<!-- the library's papers of the same item (its DOI, or its title and year), each to show, after the .bib it is in
     when the library has several (`libraryOf`): the details' -->
{#snippet inLibrary(ids: string[], onentry: (id: string) => void, libraryOf: (id: string) => string | null)}
  {#each ids as id, i (id)}
    {@const bib = libraryOf(id)}
    {#if bib || i === 0}<span class="badge lib">{bib ? `in ${bib}` : "in library"}</span>{/if}
    <button class="link" onclick={() => onentry(id)}>show</button>
  {/each}
{/snippet}

<!-- the interests in the order of their scaled scores, the best first (the keywords part is the best's) -->
<section>
  <h5>Why it is here · score {part(detail.score)}</h5>
  {#if detail.excluded !== null}
    <p>Excluded: {detail.excluded}</p>
  {/if}
  <p>Keywords {part(detail.keywords)}{#if detail.keywords === null}<span class="muted">{` · ${NO_INTERESTS}`}</span>{/if}</p>
  {#each detail.interests as s (s.interest)}
    <p class="interest" class:muted={!s.reached}>
      <span class="dot" style:background={colours.get(s.interest) ?? "var(--border)"}></span>
      {s.interest} {Math.round(s.scaled)}{#if s.terms.length}: {s.terms.join(", ")}{/if}{#if !s.reached} (below its minimum){/if}
    </p>
  {/each}
  <p>Library {part(detail.library)}{#if detail.library !== null}<span class="muted">{" · how close it is to your library's papers"}</span>{/if}</p>
</section>

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

{#if detail.nearest.length}
  <section class="similar">
    <h5>Similar in your library</h5>
    {#each detail.nearest as near (near.id)}
      <p>
        <button class="link plain" title="Show it in the library ({near.key})" onclick={() => onentry(near.id)}>{near.title}</button>
        <span class="muted">{near.subtitle} · {near.cosine.toFixed(2)}</span>
      </p>
    {/each}
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
  /* set apart, and quieter than the abstract */
  .similar {
    margin-top: 4px;
    padding: 8px 10px;
    border-top: none;
    border-radius: 8px;
    background: var(--sidebar);
    font-size: 11.5px;
  }
  .similar button.link.plain {
    color: var(--muted);
  }
  .interest {
    padding-left: 12px;
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
