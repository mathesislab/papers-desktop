<script lang="ts">
  // Subscribe: a feed's URL (RSS or Atom), a journal's ISSN swept in Crossref, a filter of OpenAlex, or words (an
  // OpenAlex sweep of the works whose title or abstract has them, titled as the words), with the kind of its items;
  // or one of the suggested subscriptions, in their groups, those followed already marked. At the top of
  // them, *From your library*: the journals of the library's papers, each by the suggested subscription of it, else
  // by its ISSN, those without one looked up in OpenAlex one after another while the sheet is open. *Search journals*
  // (or Enter in the filter) lists OpenAlex's journals of the text. A journal of these two groups is followed by the
  // feed its home page links to, read first (the journal's: its link or its own title names the journal); when its
  // site or its feed refuses, it links to none, or only to feeds that do not name the journal, nothing is subscribed:
  // the sheet says why, and offers its site (to find the feed and paste it above), a sweep of its ISSN in Crossref,
  // or the feed that may not be the journal's. What is subscribed to is fetched when the sheet closes. *Subscribed*
  // lists the subscriptions, in their groups; − next to a ✓ ends one at once (its items stay until they are old),
  // which can be subscribed to again while the sheet is open.

  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { holdModal } from "$lib/library/modal.svelte";
  import * as api from "./api";
  import { SOURCE_KINDS, sourceKinds } from "./kinds";
  import type { Follow, Journal, LibraryJournal, SourceKind, Suggested, SubscriptionInfo } from "./api";
  import {
    choiceFeed,
    feedOf,
    SOURCE_GROUPS,
    groupOf,
    journalChoice,
    journalFacts,
    journalSubscription,
    sameFeed,
    subscriptionOf,
    feedText,
    type FeedType,
    type JournalChoice,
  } from "./subscriptions";

  let {
    subscriptions,
    onsubscribed,
    onunsubscribed,
    onclose,
  }: {
    /** the subscriptions already there */
    subscriptions: SubscriptionInfo[];
    onsubscribed: (info: SubscriptionInfo) => void;
    /** a subscription ended, by its id */
    onunsubscribed: (id: string) => void;
    onclose: () => void;
  } = $props();

  const TYPES: { type: FeedType; name: string; placeholder: string }[] = [
    { type: "url", name: "Feed URL", placeholder: "https://… the RSS or Atom feed of a journal, a site, a blog" },
    { type: "issn", name: "Crossref ISSN", placeholder: "1994-0424: the works of a journal, as Crossref indexes them" },
    { type: "openalex", name: "OpenAlex filter", placeholder: "topics.id:T10017: a filter of OpenAlex works (its key helps)" },
    {
      type: "words",
      name: "Words",
      placeholder:
        "ice sheet AND (tipping OR collapse): new papers whose title or abstract has them (OpenAlex key helps)",
    },
  ];

  let type = $state<FeedType>("url");
  let text = $state("");
  /** the kind chosen; "guess": a feed read first, its kind guessed (`guessKind`); a sweep's items are papers */
  let kind = $state<SourceKind | "guess">("guess");
  let busy = $state(false);
  let message = $state<{ text: string; error?: boolean } | null>(null);
  let suggestions = $state<Suggested[] | null>(null);
  /** the journals of the library; a lookup that got no answer is `failed` */
  let library = $state<(Omit<LibraryJournal, "follow"> & { follow: Follow | { by: "failed"; message: string } })[]>(
    [],
  );
  let open = true;
  /** the journals of the last search, null while it runs */
  let found = $state<{ text: string; journals: Journal[] | null; error?: string } | null>(null);
  /** what subscribing to a journal came to, by its ISSN (else title): its site read, then its feed or the choice */
  let chosen = $state<Record<string, { reading: true } | JournalChoice>>({});
  let filter = $state("");
  /** the list below: the suggestions, or the subscriptions */
  let listing = $state<"suggested" | "subscribed">("suggested");
  /** the subscriptions ended in the sheet: listed still, to subscribe to again */
  let ended = $state<SubscriptionInfo[]>([]);
  let input = $state<HTMLInputElement>();
  let filterInput = $state<HTMLInputElement>();

  let words = $derived(filter.trim().toLowerCase());
  let groups = $derived.by(() => {
    const byGroup = new Map<string, Suggested[]>();
    for (const s of suggestions ?? []) {
      if (words && !`${s.title} ${s.group}`.toLowerCase().includes(words)) continue;
      byGroup.set(s.group, [...(byGroup.get(s.group) ?? []), s]);
    }
    return [...byGroup];
  });
  let fromLibrary = $derived(library.filter((j) => !words || j.title.toLowerCase().includes(words)));
  /** the subscriptions and those ended here, in the groups of the sidebar, by title */
  let mine = $derived.by(() => {
    const all = [...subscriptions, ...ended.filter((e) => !subscriptionOf(e.feed, subscriptions))];
    const shown = all.filter((s) => !words || `${s.title} ${feedText(s.feed)}`.toLowerCase().includes(words));
    const byTitle = (a: SubscriptionInfo, b: SubscriptionInfo) => a.title.localeCompare(b.title, undefined, { sensitivity: "base" });
    return SOURCE_GROUPS.map((g) => ({ ...g, list: shown.filter((s) => groupOf(s) === g.group).sort(byTitle) })).filter(
      (g) => g.list.length,
    );
  });

  onMount(holdModal);

  onMount(async () => {
    input?.focus();
    try {
      suggestions = await api.suggested();
    } catch (e) {
      suggestions = [];
      message = { text: String(e), error: true };
    }
  });

  onMount(() => {
    lookUpLibrary();
    return () => (open = false);
  });

  /** The journals of the library, then those pending looked up, one at a time, while the sheet is open. */
  async function lookUpLibrary() {
    try {
      library = await api.librarySuggested();
    } catch (e) {
      message = { text: String(e), error: true };
      return;
    }
    for (const journal of library) {
      if (!open) return;
      if (journal.follow.by !== "pending") continue;
      try {
        journal.follow = await api.lookupJournal(journal.title);
      } catch (e) {
        journal.follow = { by: "failed", message: String(e) };
      }
    }
  }

  /** OpenAlex's journals of the text of the filter. */
  async function searchJournals() {
    const text = filter.trim();
    if (!text) return;
    found = { text, journals: null };
    try {
      const journals = await api.searchJournals(text);
      if (found?.text === text) found.journals = journals;
    } catch (e) {
      if (found?.text === text) found = { text, journals: [], error: String(e) };
    }
  }

  const journalKey = (journal: Journal) => journal.issn ?? journal.title;

  /** A journal by the feed its site links to; else nothing, and the choice is shown. */
  async function subscribeJournal(journal: Journal, title: string) {
    if (busy) return;
    const key = journalKey(journal);
    busy = true;
    message = null;
    chosen[key] = { reading: true };
    let choice: JournalChoice;
    try {
      choice = journalChoice(await api.journalFeed(journal));
    } catch (e) {
      choice = { reason: String(e), homepage: journal.homepage, other: null };
    }
    chosen[key] = choice;
    busy = false;
    if ("feed" in choice) {
      await subscribe({ url: choice.feed }, "journal", title, `by the feed of its site, ${choice.feed}`);
    }
  }

  /** A subscription ended at once: listed still, to subscribe to again. */
  async function unsubscribe(info: SubscriptionInfo) {
    if (busy) return;
    busy = true;
    message = null;
    try {
      await api.unsubscribe(info.id);
      onunsubscribed(info.id);
      ended = [...ended.filter((e) => !sameFeed(e.feed, info.feed)), info];
      message = { text: `Unsubscribed from ${info.title}: its items stay until they are old` };
    } catch (e) {
      message = { text: String(e), error: true };
    }
    busy = false;
  }

  /** A subscription ended here, subscribed to again as it was: its title, kind, journal, pages and boost. */
  async function resubscribe(old: SubscriptionInfo) {
    const { journal, fetchPages, boost } = old;
    await subscribe(old.feed, old.kind, old.title, undefined, { journal, fetchPages, boost });
  }

  /** With `patch`, set on the subscription made. */
  async function subscribe(feed: api.Feed, as: SourceKind, title?: string, how?: string, patch?: api.SubscriptionPatch) {
    if (busy) return;
    if (subscriptions.some((s) => sameFeed(s.feed, feed))) {
      message = { text: "Subscribed already" };
      return;
    }
    busy = true;
    message = null;
    try {
      let info = await api.subscribe(feed, as, title);
      if (patch) info = await api.updateSubscription(info.id, patch);
      ended = ended.filter((e) => !sameFeed(e.feed, feed));
      onsubscribed(info);
      message = { text: `Subscribed to ${info.title}${how ? ` ${how}` : ""}: fetched when the sheet closes` };
      text = "";
    } catch (e) {
      message = { text: String(e), error: true };
    }
    busy = false;
  }

  async function subscribeTyped() {
    const feed = feedOf(type, text);
    if (!feed || busy) return;
    const title = type === "words" ? text.trim() : undefined;
    if (kind !== "guess") return subscribe(feed, kind, title);
    if (!("url" in feed)) return subscribe(feed, "journal", title);
    busy = true;
    message = { text: "Reading the feed…" };
    let guess: api.Guess;
    try {
      guess = await api.guessKind(feed.url);
    } catch (e) {
      message = { text: String(e), error: true };
      busy = false;
      return;
    }
    busy = false;
    const name = SOURCE_KINDS[guess.kind].name;
    await subscribe(feed, guess.kind, undefined, `as ${name} (${guess.reason}; right-click an item to change it)`);
  }

  function onkeydown(e: KeyboardEvent) {
    // the keys of the feeds (j, k, s, the arrows) are not for the sheet
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "Enter" && e.target === input) {
      e.preventDefault();
      subscribeTyped();
    } else if (e.key === "Enter" && e.target === filterInput) {
      e.preventDefault();
      searchJournals();
    }
  }
</script>

<!-- ✓ and − for a subscription there; else Subscribe -->
{#snippet subscribed(sub: SubscriptionInfo)}
  <span class="on">✓ Subscribed</span>
  <button class="minus" disabled={busy} title="Unsubscribe from {sub.title}" aria-label="Unsubscribe from {sub.title}" onclick={() => unsubscribe(sub)}>
    −
  </button>
{/snippet}

{#snippet follow(feed: api.Feed, as: SourceKind)}
  {@const sub = subscriptionOf(feed, subscriptions)}
  {#if sub}
    {@render subscribed(sub)}
  {:else}
    <button disabled={busy} onclick={() => subscribe(feed, as)}>Subscribe</button>
  {/if}
{/snippet}

<!-- a journal of the library or of a search: by the feed of its site, else the choice of its site or Crossref -->
{#snippet journalRow(title: string, facts: string, journal: Journal)}
  {@const state = chosen[journalKey(journal)]}
  {@const shown = choiceFeed(state)}
  {@const sub = journalSubscription(journal, subscriptions) ?? (shown ? subscriptionOf({ url: shown }, subscriptions) : undefined)}
  {@const on = !!sub}
  <div class="suggestion">
    <span class="title" title={title}>{title}</span>
    <span class="muted">{facts}</span>
    {#if sub}
      {@render subscribed(sub)}
    {:else if !journal.issn}
      <span class="muted">no ISSN found</span>
    {:else if state && "reading" in state}
      <span class="muted">reading its site…</span>
    {:else}
      <button disabled={busy} onclick={() => subscribeJournal(journal, title)}>Subscribe</button>
    {/if}
  </div>
  {#if !on && journal.issn && state && "reason" in state}
    {@const issn = journal.issn}
    <div class="choice">
      <span class="muted">{state.reason}</span>
      <span class="spacer"></span>
      {#if state.other}
        {@const other = state.other.url}
        <button disabled={busy} onclick={() => subscribe({ url: other }, "journal")}>Subscribe to it</button>
      {/if}
      {#if state.homepage}
        {@const homepage = state.homepage}
        <button onclick={() => openUrl(homepage).catch((e) => (message = { text: String(e), error: true }))}>
          Open its site
        </button>
      {/if}
      <button disabled={busy} onclick={() => subscribe({ issn }, "journal", title)}>
        Follow in Crossref
      </button>
    </div>
  {/if}
{/snippet}

<div class="backdrop" role="presentation" {onkeydown}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="subscribe-title">
    <h2 id="subscribe-title">Subscribe</h2>

    <div class="row">
      <span class="modes" role="group" aria-label="What to follow">
        {#each TYPES as t (t.type)}
          <button class:active={type === t.type} onclick={() => ((type = t.type), input?.focus())}>{t.name}</button>
        {/each}
      </span>
      <span class="spacer"></span>
      <label class="kind">
        Its items are
        <select bind:value={kind}>
          <option value="guess">{type === "url" ? "Guessed from the feed" : "Papers"}</option>
          {#each sourceKinds as kind (kind)}{@const k = SOURCE_KINDS[kind]}<option value={kind}>{k.name}{k.hint ? ` (${k.hint})` : ""}</option>{/each}
        </select>
      </label>
    </div>
    <div class="row">
      <input
        bind:this={input}
        bind:value={text}
        type="text"
        placeholder={TYPES.find((t) => t.type === type)?.placeholder}
        spellcheck="false"
        autocomplete="off"
      />
      <button class="primary" disabled={busy || !text.trim()} onclick={subscribeTyped}>Subscribe</button>
    </div>
    {#if message}
      <p class="said" class:error={message.error}>{message.text}</p>
    {/if}

    <div class="suggested">
      <div class="row">
        <span class="modes" role="group" aria-label="What to list">
          <button class:active={listing === "suggested"} onclick={() => (listing = "suggested")}>Suggested</button>
          <button class:active={listing === "subscribed"} onclick={() => (listing = "subscribed")}>
            Subscribed ({subscriptions.length})
          </button>
        </span>
        <span class="spacer"></span>
        <input
          class="filter"
          bind:this={filterInput}
          bind:value={filter}
          type="search"
          placeholder={listing === "suggested" ? "Filter, or a journal to search" : "Filter"}
          spellcheck="false"
          autocomplete="off"
        />
        {#if listing === "suggested"}
          <button disabled={!filter.trim()} onclick={searchJournals}>Search journals</button>
        {/if}
      </div>
      <div class="list">
        {#if listing === "subscribed"}
          {#each mine as g (g.group)}
            <h4>{g.name}</h4>
            {#each g.list as s (s.id)}
              {@const sub = subscriptionOf(s.feed, subscriptions)}
              <div class="suggestion">
                <span class="title" title={s.title}>{s.title}</span>
                <span class="muted feed" title={feedText(s.feed)}>{feedText(s.feed)}</span>
                {#if sub}
                  {@render subscribed(sub)}
                {:else}
                  <button disabled={busy} onclick={() => resubscribe(s)}>Subscribe</button>
                {/if}
              </div>
            {/each}
          {:else}
            <p class="muted">{subscriptions.length ? "None" : "No subscriptions yet"}</p>
          {/each}
        {:else}
        {#if found}
          <h4>Journals matching "{found.text}"</h4>
          {#if found.journals === null}
            <p class="muted">Searching…</p>
          {:else if found.error}
            <p class="muted error">{found.error}</p>
          {:else if !found.journals.length}
            <p class="muted">None</p>
          {/if}
          {#each found.journals ?? [] as journal}
            {@render journalRow(journal.title, journalFacts(journal), journal)}
          {/each}
        {/if}
        {#if fromLibrary.length}
          <h4>From your library</h4>
          {#each fromLibrary as j (j.title)}
            {@const f = j.follow}
            {#if f.by === "journal"}
              {@render journalRow(j.title, `${j.papers} papers`, f)}
            {:else}
              <div class="suggestion">
                <span class="title" title={j.title}>{j.title}</span>
                <span class="muted">{j.papers} papers</span>
                {#if f.by === "suggested"}
                  {@render follow(f.feed, f.kind)}
                {:else if f.by === "pending"}
                  <span class="muted">looking up…</span>
                {:else if f.by === "failed"}
                  <span class="muted" title={f.message}>not looked up</span>
                {:else}
                  <span class="muted">no ISSN found</span>
                {/if}
              </div>
            {/if}
          {/each}
        {/if}
        {#if suggestions === null}
          <p class="muted">Loading…</p>
        {:else if !groups.length && !fromLibrary.length}
          <p class="muted">None</p>
        {/if}
        {#each groups as [group, list] (group)}
          <h4>{group}</h4>
          {#each list as s (s.title)}
            <div class="suggestion">
              <span class="title" title={s.title}>{s.title}</span>
              <span class="muted">{SOURCE_KINDS[s.kind].name}</span>
              {@render follow(s.feed, s.kind)}
            </div>
          {/each}
        {/each}
        {/if}
      </div>
    </div>

    <div class="actions">
      <span class="spacer"></span>
      <button class="primary" onclick={onclose}>Done</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 9;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    box-sizing: border-box;
    padding: max(24px, 9vh) 16px 16px;
    background: rgba(0, 0, 0, 0.25);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    width: min(640px, 100%);
    max-height: 100%;
    box-sizing: border-box;
    padding: 18px 20px 16px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--menu-bg);
    color: var(--fg);
    font-size: 13px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.22);
  }
  h2 {
    margin: 0 0 10px;
    font-size: 14px;
    font-weight: 600;
  }
  h4 {
    margin: 10px 0 4px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 26px;
    margin-top: 6px;
  }
  .spacer {
    flex: 1;
  }
  input {
    flex: 1;
    min-width: 0;
    box-sizing: border-box;
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--fg);
    font: inherit;
    outline: none;
    -webkit-user-select: text;
    user-select: text;
  }
  input:focus {
    border-color: var(--accent);
  }
  input.filter {
    flex: 0 1 180px;
    font-size: 12px;
  }
  .kind {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
    font-size: 12px;
  }
  select {
    font: inherit;
    font-size: 12px;
    color: var(--fg);
    background: var(--menu-bg);
  }
  .said {
    margin: 6px 0 0;
    color: var(--muted);
    overflow-wrap: anywhere;
    -webkit-user-select: text;
    user-select: text;
  }
  .said.error {
    color: var(--error);
  }
  .suggested {
    display: flex;
    flex-direction: column;
    min-height: 0;
    margin-top: 14px;
    padding-top: 8px;
    border-top: 1px solid var(--line);
  }
  .list {
    min-height: 120px;
    max-height: 42vh;
    overflow-y: auto;
  }
  .suggestion {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 26px;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 26px;
    padding-left: 12px;
  }
  .choice .muted {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .suggestion .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .muted {
    color: var(--muted);
    font-size: 12px;
  }
  .muted.error {
    color: var(--error);
  }
  .on {
    color: #2f9e6e;
    font-size: 12px;
  }
  /* the end of a subscription, next to its ✓ */
  button.minus {
    width: 22px;
    margin-left: -4px;
    padding: 1px 0;
    color: var(--muted);
    line-height: 1;
  }
  button.minus:hover:not(:disabled) {
    border-color: var(--error);
    color: var(--error);
  }
  .muted.feed {
    max-width: 40%;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 14px;
  }
  button {
    flex: none;
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
  .modes {
    display: inline-flex;
  }
  .modes button {
    margin-left: -1px;
    border-radius: 0;
  }
  .modes button:first-child {
    margin-left: 0;
    border-radius: 6px 0 0 6px;
  }
  .modes button:last-child {
    border-radius: 0 6px 6px 0;
  }
  .modes button.active {
    position: relative;
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
</style>
