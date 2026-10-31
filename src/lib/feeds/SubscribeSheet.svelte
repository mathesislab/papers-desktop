<script lang="ts">
  // Subscribe: a feed's URL (RSS or Atom), a journal's ISSN swept in Crossref, or a filter of OpenAlex, with the kind
  // of its items; or one of the suggested subscriptions, in their groups, those followed already marked. What is
  // subscribed to is fetched when the sheet closes.

  import { onMount } from "svelte";
  import { holdModal } from "$lib/library/modal.svelte";
  import * as api from "./api";
  import type { SourceKind, Suggested, SubscriptionInfo } from "./api";
  import { feedOf, sameFeed, type FeedType } from "./filters";

  let {
    subscriptions,
    onsubscribed,
    onclose,
  }: {
    /** the subscriptions already there */
    subscriptions: SubscriptionInfo[];
    onsubscribed: (info: SubscriptionInfo) => void;
    onclose: () => void;
  } = $props();

  const TYPES: { type: FeedType; name: string; placeholder: string }[] = [
    { type: "rss", name: "Feed URL", placeholder: "https://… the RSS or Atom feed of a journal, a site, a blog" },
    { type: "crossref", name: "Crossref ISSN", placeholder: "1994-0424: the works of a journal, as Crossref indexes them" },
    { type: "openalex", name: "OpenAlex filter", placeholder: "topics.id:T10017: a filter of OpenAlex works (needs its key)" },
  ];
  const KINDS: { kind: SourceKind; name: string }[] = [
    { kind: "journal", name: "Journal" },
    { kind: "preprints", name: "Preprint server" },
    { kind: "news", name: "News & press" },
    { kind: "blog", name: "Blog" },
  ];

  let type = $state<FeedType>("rss");
  let text = $state("");
  let kind = $state<SourceKind>("journal");
  let busy = $state(false);
  let message = $state<{ text: string; error?: boolean } | null>(null);
  let suggestions = $state<Suggested[] | null>(null);
  let filter = $state("");
  let input = $state<HTMLInputElement>();

  let groups = $derived.by(() => {
    const words = filter.trim().toLowerCase();
    const byGroup = new Map<string, Suggested[]>();
    for (const s of suggestions ?? []) {
      if (words && !`${s.title} ${s.group}`.toLowerCase().includes(words)) continue;
      byGroup.set(s.group, [...(byGroup.get(s.group) ?? []), s]);
    }
    return [...byGroup];
  });

  const followed = (s: Suggested) => subscriptions.some((sub) => sameFeed(sub.feed, s.feed));

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

  async function subscribe(feed: api.Feed, as: SourceKind) {
    if (busy) return;
    if (subscriptions.some((s) => sameFeed(s.feed, feed))) {
      message = { text: "Subscribed already" };
      return;
    }
    busy = true;
    message = null;
    try {
      const info = await api.subscribe(feed, as);
      onsubscribed(info);
      message = { text: `Subscribed to ${info.title}: fetched when the sheet closes` };
      text = "";
    } catch (e) {
      message = { text: String(e), error: true };
    }
    busy = false;
  }

  function subscribeTyped() {
    const feed = feedOf(type, text);
    if (feed) subscribe(feed, kind);
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
    }
  }
</script>

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
          {#each KINDS as k (k.kind)}<option value={k.kind}>{k.name}</option>{/each}
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
        <h3>Suggested</h3>
        <span class="spacer"></span>
        <input class="filter" bind:value={filter} type="search" placeholder="Filter" spellcheck="false" autocomplete="off" />
      </div>
      <div class="list">
        {#if suggestions === null}
          <p class="muted">Loading…</p>
        {:else if !groups.length}
          <p class="muted">None</p>
        {/if}
        {#each groups as [group, list] (group)}
          <h4>{group}</h4>
          {#each list as s (s.title)}
            {@const on = followed(s)}
            <div class="suggestion">
              <span class="title" title={s.title}>{s.title}</span>
              <span class="muted">{KINDS.find((k) => k.kind === s.kind)?.name ?? s.kind}</span>
              {#if on}
                <span class="on">✓ Subscribed</span>
              {:else}
                <button disabled={busy} onclick={() => subscribe(s.feed, s.kind)}>Subscribe</button>
              {/if}
            </div>
          {/each}
        {/each}
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
  h3 {
    margin: 0;
    font-size: 12px;
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
  .on {
    color: #2f9e6e;
    font-size: 12px;
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
