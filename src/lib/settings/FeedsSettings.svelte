<script lang="ts">
  // The Feeds tab of the settings window: the subscriptions (added by
  // address, ISSN or OpenAlex filter, or from the suggested list; edited;
  // OPML in and out), the rules, the days kept, the background polling; the switches of the main window (whether
  // the Feeds and Search tabs show, the details beside the list). The interests are the Score tab's
  // (ScoreSettings); both save over the feeds' settings as saved (`FeedPrefs`). The commands are those of
  // $lib/feeds/api.ts and, for the switches, $lib/toggles.ts.
  import { open, save as saveFile } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import * as feeds from "$lib/feeds/api";
  import type { Feed, FeedSettings, Rule, SourceKind, Suggested, SubscriptionInfo } from "$lib/feeds/api";
  import { since } from "$lib/feeds/items";
  import { SOURCE_KINDS, sourceKinds } from "$lib/feeds/kinds";
  import { feedOf, feedText, sameFeed, subscriptionOf, type FeedType } from "$lib/feeds/subscriptions";
  import { onToggle, setToggle, toggles, type Toggle, type Toggles } from "$lib/toggles";
  import type { FeedPrefs } from "./feeds.svelte";
  import "./settings.css";

  let { feedPrefs, notify }: { feedPrefs: FeedPrefs; notify: (text: string, error: boolean) => void } = $props();

  const TYPES: { value: FeedType; name: string; placeholder: string }[] = [
    { value: "url", name: "Feed", placeholder: "https://…/rss" },
    { value: "issn", name: "ISSN", placeholder: "0022-1430" },
    { value: "openalex", name: "OpenAlex", placeholder: "primary_topic.id:T10644" },
  ];

  let subscriptions = $state<SubscriptionInfo[]>([]);
  /** the settings as saved */
  let saved = $derived(feedPrefs.saved);
  /** the switches of the main window, also set by the Window menu */
  let switches = $state<Toggles>({ "feeds-tab": true, "search-tab": true, "feeds-beside": false });
  const SWITCHES: { toggle: Toggle; label: string; hint: string }[] = [
    { toggle: "feeds-tab", label: "Show the Feeds tab", hint: "In the main window; also Window → Show Feeds Tab" },
    {
      toggle: "search-tab",
      label: "Show the Search tab",
      hint: "In the main window, to search OpenAlex's works and add them; also Window → Show Search Tab",
    },
    {
      toggle: "feeds-beside",
      label: "Details beside the list",
      hint: "Otherwise an item opens in place of the list, to read (Esc goes back), in the Feeds and Search tabs; also Window → Feeds Details Beside the List",
    },
  ];
  /** the rules being edited */
  let draft = $state<Rule[] | null>(null);
  let draftError = $state("");
  let suggestions = $state<Suggested[]>([]);
  let showSuggested = $state(false);
  /** the suggested subscriptions ticked, by their feeds */
  let picked = $state<Feed[]>([]);
  let busy = $state(false);

  // a new subscription
  let addType = $state<FeedType>("url");
  let addText = $state("");
  let addKind = $state<SourceKind>("journal");

  /** the subscription being edited, and its fields */
  let editing = $state<string | null>(null);
  let edit = $state({ title: "", kind: "journal" as SourceKind, journal: "", fetchPages: false, boost: 0 as number | null });

  // the rules to edit, once the settings are loaded (by the page)
  $effect(() => {
    if (saved && !draft) draft = toDraft(saved);
  });

  onMount(() => {
    load();
    const unlisten = listen(feeds.EVENTS.changed, () => loadSubscriptions());
    const unlistenToggle = onToggle((toggle, on) => (switches[toggle] = on));
    return () => {
      unlisten.then((f) => f());
      unlistenToggle();
    };
  });

  async function load() {
    try {
      await Promise.all([loadSubscriptions(), toggles().then((on) => (switches = on))]);
      suggestions = await feeds.suggested();
    } catch (err) {
      notify(String(err), true);
    }
  }

  async function loadSubscriptions() {
    subscriptions = await feeds.subscriptions();
  }

  /** The rules as saved, those edited dropped. */
  function revert() {
    if (saved) draft = toDraft(saved);
    draftError = "";
  }

  /** When a subscription was last fetched, or why it failed. */
  function lastText(s: SubscriptionInfo): string {
    if (!s.last) return "Not fetched yet";
    if (s.last.error) return `Failed: ${s.last.error}`;
    return `Fetched ${since(s.last.at)}`;
  }

  async function act(work: () => Promise<unknown>, done?: string) {
    busy = true;
    try {
      await work();
      if (done) notify(done, false);
    } catch (err) {
      notify(String(err), true);
    } finally {
      busy = false;
    }
  }

  /** A subscription typed, as the Subscribe sheet makes one: none for what is followed already. */
  function add() {
    const feed = feedOf(addType, addText);
    if (!feed) return;
    if (subscriptionOf(feed, subscriptions)) return notify("Subscribed already", false);
    act(async () => {
      await feeds.subscribe(feed, addKind);
      addText = "";
      await loadSubscriptions();
    }, "Subscribed");
  }

  function remove(s: SubscriptionInfo) {
    act(async () => {
      await feeds.unsubscribe(s.id);
      await loadSubscriptions();
    }, "Unsubscribed; its items stay until they are old");
  }

  function startEdit(s: SubscriptionInfo) {
    editing = s.id;
    edit = { title: s.title, kind: s.kind, journal: s.journal ?? "", fetchPages: s.fetchPages, boost: s.boost };
  }

  function saveEdit(id: string) {
    act(async () => {
      await feeds.updateSubscription(id, {
        title: edit.title,
        kind: edit.kind,
        journal: edit.journal.trim() || null,
        fetchPages: edit.fetchPages,
        boost: edit.boost ?? 0,
      });
      editing = null;
      await loadSubscriptions();
    }, "Saved");
  }

  const isPicked = (feed: Feed) => picked.some((f) => sameFeed(f, feed));
  let groups = $derived.by(() => {
    const by = new Map<string, Suggested[]>();
    for (const s of suggestions) by.set(s.group, [...(by.get(s.group) ?? []), s]);
    return [...by.entries()];
  });

  function toggle(s: Suggested) {
    picked = isPicked(s.feed) ? picked.filter((f) => !sameFeed(f, s.feed)) : [...picked, s.feed];
  }

  /** The suggested subscriptions ticked, each with its title, as the Subscribe sheet makes them. */
  function subscribeSuggested() {
    const chosen = suggestions.filter((s) => isPicked(s.feed) && !subscriptionOf(s.feed, subscriptions));
    act(async () => {
      for (const s of chosen) await feeds.subscribe(s.feed, s.kind, s.title);
      picked = [];
      showSuggested = false;
      await loadSubscriptions();
    }, `Subscribed to ${chosen.length}`);
  }

  async function importOpml() {
    const file = await open({ title: "Import subscriptions", filters: [{ name: "OPML", extensions: ["opml", "xml"] }] });
    if (typeof file !== "string") return;
    await act(async () => {
      const added = await feeds.importOpml(file);
      await loadSubscriptions();
      notify(`${added} new subscription${added === 1 ? "" : "s"}`, false);
    });
  }

  async function exportOpml() {
    const file = await saveFile({ title: "Export subscriptions", defaultPath: "feeds.opml", filters: [{ name: "OPML", extensions: ["opml"] }] });
    if (file) await act(() => feeds.exportOpml(file), "Exported");
  }

  // the rules: edited as a draft, saved together

  const toDraft = (s: FeedSettings): Rule[] => s.rules.map((r) => ({ ...r }));

  /** The rules edited, as saved: a rule with no query would match every item, left out. */
  const fromDraft = (rules: Rule[]): Rule[] => rules.filter((r) => r.when.trim()).map((r) => ({ when: r.when.trim(), then: r.then }));

  let dirty = $derived(!!saved && !!draft && JSON.stringify(fromDraft(draft)) !== JSON.stringify(saved.rules));

  async function saveDraft() {
    if (!draft) return;
    busy = true;
    try {
      await feedPrefs.save({ rules: fromDraft(draft) });
      revert();
      notify("Saved", false);
    } catch (err) {
      draftError = String(err);
      notify(draftError, true);
    } finally {
      busy = false;
    }
  }

  /** A setting that saves at once: with the rules as saved. */
  const setNow = (change: Partial<FeedSettings>) => act(() => feedPrefs.save(change), "Saved");

  const number = (e: Event) => Number((e.currentTarget as HTMLInputElement).value);
</script>

<section>
  <h2>Feeds</h2>

  {#each SWITCHES as s (s.toggle)}
    <label class="row">
      <span>
        <span class="label">{s.label}</span>
        <span class="hint">{s.hint}</span>
      </span>
      <input type="checkbox" class="switch" checked={switches[s.toggle]} onchange={(e) => act(() => setToggle(s.toggle, e.currentTarget.checked))} />
    </label>
  {/each}

  <ul class="subs">
    {#each subscriptions as s (s.id)}
      <li>
        <div class="sub">
          <span class="main">
            <span class="title" title={s.title}>{s.title}</span>
            <span class="hint" title={feedText(s.feed)}>{SOURCE_KINDS[s.kind].name} · {feedText(s.feed)}{s.boost ? ` · boost ${s.boost > 0 ? "+" : ""}${s.boost}` : ""}</span>
            <span class="hint" class:error={!!s.last?.error}>{lastText(s)}</span>
          </span>
          <button class="small" onclick={() => (editing === s.id ? (editing = null) : startEdit(s))}>Edit</button>
          <button class="remove" title="Unsubscribe" disabled={busy} onclick={() => remove(s)}>✕</button>
        </div>
        {#if editing === s.id}
          <div class="editor">
            <label>Title <input class="text" type="text" bind:value={edit.title} placeholder="The feed's own" /></label>
            <label>Kind
              <select bind:value={edit.kind}>
                {#each sourceKinds as kind (kind)}<option value={kind}>{SOURCE_KINDS[kind].name}</option>{/each}
              </select>
            </label>
            <label>Journal <input class="text" type="text" bind:value={edit.journal} placeholder="As the feed says" /></label>
            <label title="Added to the score of its items in each interest (a veto stays 0); 2 for “cites my work”, so that a weak match is enough">Boost <input class="num" type="number" step="0.5" bind:value={edit.boost} /></label>
            <label class="check"><input type="checkbox" bind:checked={edit.fetchPages} /> Read each item's page at fetch time (the paper of a post)</label>
            <span class="buttons">
              <button class="small" onclick={() => (editing = null)}>Cancel</button>
              <button class="small primary" disabled={busy} onclick={() => saveEdit(s.id)}>Save</button>
            </span>
          </div>
        {/if}
      </li>
    {:else}
      <li class="none">No subscription: add a feed, or pick from the suggested ones.</li>
    {/each}
  </ul>

  <div class="add">
    <select bind:value={addType} title="What it follows">
      {#each TYPES as t (t.value)}<option value={t.value}>{t.name}</option>{/each}
    </select>
    <input
      class="text grow"
      type="text"
      spellcheck="false"
      autocapitalize="off"
      placeholder={TYPES.find((t) => t.value === addType)?.placeholder}
      bind:value={addText}
      onkeydown={(e) => e.key === "Enter" && add()}
    />
    <select bind:value={addKind} title="The kind of its items">
      {#each sourceKinds as kind (kind)}<option value={kind}>{SOURCE_KINDS[kind].name}</option>{/each}
    </select>
    <button disabled={busy || !addText.trim()} onclick={add}>Add</button>
  </div>
  <p class="hint">An ISSN is swept in Crossref; an OpenAlex filter in OpenAlex, which its key helps.</p>

  <div class="actions">
    <button onclick={() => (showSuggested = !showSuggested)}>{showSuggested ? "Hide Suggested" : "Suggested…"}</button>
    <button onclick={importOpml} disabled={busy}>Import OPML…</button>
    <button onclick={exportOpml} disabled={busy || !subscriptions.length}>Export OPML…</button>
  </div>

  {#if showSuggested}
    <div class="suggested">
      {#each groups as [group, list] (group)}
        <div class="group">
          <span class="label">{group}</span>
          {#each list as s (feedText(s.feed))}
            {@const has = !!subscriptionOf(s.feed, subscriptions)}
            <label class="check" class:muted={has}>
              <input type="checkbox" checked={has || isPicked(s.feed)} disabled={has} onchange={() => toggle(s)} />
              {s.title}{#if s.kind !== "journal"}<span class="hint inline"> · {SOURCE_KINDS[s.kind].name}</span>{/if}
            </label>
          {/each}
        </div>
      {/each}
      <button class="primary" disabled={busy || !picked.length} onclick={subscribeSuggested}>
        Subscribe to {picked.length || ""} Selected
      </button>
    </div>
  {/if}

  {#if draft && saved}
    <h3>Rules</h3>
    <p class="hint">Applied to the new items of a fetch, the first that matches deciding: a query of the feeds (<code>prof:amoc score:80-</code>, the score 0–100 of Settings → Score; <code>t:editorial</code>) and what to do.</p>
    {#each draft as rule, i (i)}
      <div class="line">
        <input class="text grow" type="text" spellcheck="false" placeholder="Query" bind:value={rule.when} />
        <select bind:value={rule.then}>
          <option value="star">Star</option>
          <option value="seen">Mark seen</option>
          <option value="remove">Remove</option>
        </select>
        <button class="remove" onclick={() => draft!.splice(i, 1)}>✕</button>
      </div>
    {/each}
    <div class="actions">
      <button class="small" onclick={() => draft!.push({ when: "", then: "star" })}>Add Rule</button>
    </div>

    {#if dirty || draftError}
      <div class="save">
        <span class="hint" class:error={!!draftError}>{draftError || "Rules changed"}</span>
        <span class="buttons">
          <button class="small" onclick={revert}>Revert</button>
          <button class="small primary" disabled={busy || !dirty} onclick={saveDraft}>Save</button>
        </span>
      </div>
    {/if}

    <h3>Fetching</h3>
    <label class="row">
      <span>
        <span class="label">Fetch in the background</span>
        <span class="hint">Otherwise when the Feeds tab opens, and on {"⌘R"}</span>
      </span>
      <input type="checkbox" class="switch" checked={saved.poll} onchange={(e) => setNow({ poll: e.currentTarget.checked })} />
    </label>
    {#if saved.poll}
      <label class="row">
        <span class="label">Every (minutes)</span>
        <input class="num" type="number" min="5" value={saved.pollMinutes} onchange={(e) => setNow({ pollMinutes: number(e) })} />
      </label>
    {/if}
    <label class="row">
      <span>
        <span class="label">Keep items (days)</span>
        <span class="hint">An item no feed lists any more goes after this, unless starred or added</span>
      </span>
      <input class="num" type="number" min="1" value={saved.keepDays} onchange={(e) => setNow({ keepDays: number(e) })} />
    </label>
    <label class="row">
      <span>
        <span class="label">Look back (days)</span>
        <span class="hint">How far the first sweep of an ISSN or a filter reaches</span>
      </span>
      <input class="num" type="number" min="1" value={saved.lookbackDays} onchange={(e) => setNow({ lookbackDays: number(e) })} />
    </label>
  {/if}
</section>

<style>
  p.hint {
    margin: 2px 0 6px;
  }
  .hint.inline {
    display: inline;
  }
  .hint.error {
    -webkit-user-select: text;
    user-select: text;
  }

  .subs {
    margin: 4px 0 8px;
    padding: 0;
    list-style: none;
  }
  .subs li {
    padding: 6px 0;
    border-bottom: 1px solid var(--line);
  }
  .subs .none {
    color: var(--muted);
  }
  .sub {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .sub .main {
    flex: 1;
    min-width: 0;
  }
  .sub .main > span {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .title {
    display: block;
  }
  .editor {
    display: grid;
    gap: 6px;
    margin: 8px 0 2px 12px;
  }
  .editor label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .add,
  .line,
  .actions,
  .save {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .add {
    margin-top: 4px;
  }
  .actions {
    margin: 6px 0;
    flex-wrap: wrap;
  }
  .save {
    justify-content: space-between;
    margin: 8px 0;
  }
  .line {
    margin: 4px 0;
  }

  .suggested {
    margin: 4px 0 8px;
    padding: 8px 10px;
    border: 1px solid var(--line);
    border-radius: 8px;
  }
  .group {
    margin-bottom: 8px;
  }
  .group .label {
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    margin-bottom: 2px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 1px 0;
  }
  .check.muted {
    color: var(--muted);
  }

  input.text,
  input.num {
    padding: 3px 7px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
  }
  input.text {
    width: 220px;
    min-width: 0;
  }
  input.grow {
    flex: 1;
    width: auto;
  }
  input.num {
    width: 56px;
  }
  .buttons {
    justify-content: flex-end;
  }
  select {
    max-width: 120px;
  }
  button.small {
    padding: 2px 8px;
    font-size: 12px;
  }
</style>
