<script lang="ts">
  // The Feeds section of the settings window: the subscriptions (added by
  // address, ISSN or OpenAlex filter, or from the suggested list; edited;
  // OPML in and out), the interests and the rules, the days kept, the
  // background polling. The commands are those of $lib/feeds/api.ts.
  import { open, save as saveFile } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import * as feeds from "$lib/feeds/api";
  import type { Feed, FeedSettings, Interest, Rule, SourceKind, Suggested, SubscriptionInfo } from "$lib/feeds/api";

  let { notify }: { notify: (text: string, error: boolean) => void } = $props();

  const KINDS: { value: SourceKind; name: string }[] = [
    { value: "journal", name: "Journal" },
    { value: "preprints", name: "Preprints" },
    { value: "news", name: "News" },
    { value: "blog", name: "Blog" },
  ];
  type FeedType = "rss" | "crossref" | "openalex";
  const TYPES: { value: FeedType; name: string; placeholder: string }[] = [
    { value: "rss", name: "Feed", placeholder: "https://…/rss" },
    { value: "crossref", name: "ISSN", placeholder: "0022-1430" },
    { value: "openalex", name: "OpenAlex", placeholder: "primary_topic.id:T10644" },
  ];

  let subscriptions = $state<SubscriptionInfo[]>([]);
  /** the settings as saved, and the interests and rules being edited */
  let saved = $state<FeedSettings | null>(null);
  let draft = $state<Draft | null>(null);
  let draftError = $state("");
  let suggestions = $state<Suggested[]>([]);
  let showSuggested = $state(false);
  let picked = $state<Set<string>>(new Set());
  let busy = $state(false);

  // a new subscription
  let addType = $state<FeedType>("rss");
  let addText = $state("");
  let addKind = $state<SourceKind>("journal");

  /** the subscription being edited, and its fields */
  let editing = $state<string | null>(null);
  let edit = $state({ title: "", kind: "journal" as SourceKind, journal: "", fetchPages: false, boost: 0 as number | null });

  /** an include or exclude as the editor shows it */
  type Line = { pattern: boolean; text: string; weight: number; label: string };
  type DraftInterest = { name: string; min_score: number; include: Line[]; exclude: Line[]; open: boolean };
  type Draft = { interests: DraftInterest[]; titleBoost: number; rules: Rule[] };

  onMount(() => {
    load();
    const unlisten = listen(feeds.EVENTS.changed, () => loadSubscriptions());
    return () => {
      unlisten.then((f) => f());
    };
  });

  async function load() {
    try {
      await Promise.all([loadSubscriptions(), loadSettings()]);
      suggestions = await feeds.suggested();
    } catch (err) {
      notify(String(err), true);
    }
  }

  async function loadSubscriptions() {
    subscriptions = await feeds.subscriptions();
  }

  async function loadSettings() {
    saved = await feeds.settings();
    draft = toDraft(saved);
    draftError = "";
  }

  const feedText = (feed: Feed) => ("rss" in feed ? feed.rss : "crossref" in feed ? `ISSN ${feed.crossref}` : `OpenAlex ${feed.openalex}`);
  const feedKey = (feed: Feed) => JSON.stringify(feed);
  const kindName = (kind: SourceKind) => KINDS.find((k) => k.value === kind)?.name ?? kind;

  /** When a subscription was last fetched, or why it failed. */
  function lastText(s: SubscriptionInfo): string {
    if (!s.last) return "Not fetched yet";
    if (s.last.error) return `Failed: ${s.last.error}`;
    return `Fetched ${ago(s.last.at)}`;
  }

  function ago(at: string): string {
    const minutes = Math.round((Date.now() - Date.parse(at)) / 60000);
    if (minutes < 1) return "just now";
    if (minutes < 60) return `${minutes} min ago`;
    const hours = Math.round(minutes / 60);
    return hours < 48 ? `${hours} h ago` : `${Math.round(hours / 24)} days ago`;
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

  function add() {
    const text = addText.trim();
    if (!text) return;
    const feed = { [addType]: text } as Feed;
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

  let subscribed = $derived(new Set(subscriptions.map((s) => feedKey(s.feed))));
  let groups = $derived.by(() => {
    const by = new Map<string, Suggested[]>();
    for (const s of suggestions) by.set(s.group, [...(by.get(s.group) ?? []), s]);
    return [...by.entries()];
  });

  function toggle(s: Suggested) {
    const key = feedKey(s.feed);
    const next = new Set(picked);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    picked = next;
  }

  function subscribeSuggested() {
    const chosen = suggestions.filter((s) => picked.has(feedKey(s.feed)) && !subscribed.has(feedKey(s.feed)));
    act(async () => {
      for (const s of chosen) {
        const made = await feeds.subscribe(s.feed, s.kind);
        await feeds.updateSubscription(made.id, { title: s.title });
      }
      picked = new Set();
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

  // the interests and rules: edited as a draft, saved together

  function toLine(m: { term?: string; pattern?: string; weight?: number; label?: string }): Line {
    return { pattern: m.pattern !== undefined, text: m.pattern ?? m.term ?? "", weight: m.weight ?? 1, label: m.label ?? "" };
  }

  function toDraft(s: FeedSettings): Draft {
    return {
      interests: s.interests.map((i) => ({
        name: i.name,
        min_score: i.min_score,
        include: i.include.map(toLine),
        exclude: i.exclude.map(toLine),
        open: false,
      })),
      titleBoost: s.titleBoost,
      rules: s.rules.map((r) => ({ ...r })),
    };
  }

  function fromDraft(d: Draft): Pick<FeedSettings, "interests" | "titleBoost" | "rules"> {
    const matcher = (l: Line) => (l.pattern ? { pattern: l.text.trim() } : { term: l.text.trim() });
    const interests: Interest[] = d.interests.map((i) => ({
      name: i.name.trim(),
      include: i.include
        .filter((l) => l.text.trim())
        .map((l) => ({ ...matcher(l), weight: Number(l.weight), ...(l.label.trim() ? { label: l.label.trim() } : {}) })),
      exclude: i.exclude.filter((l) => l.text.trim()).map(matcher),
      min_score: Number(i.min_score),
    }));
    // a rule with no query would match every item: left out
    const rules = d.rules.filter((r) => r.when.trim()).map((r) => ({ when: r.when.trim(), then: r.then }));
    return { interests, titleBoost: Number(d.titleBoost), rules };
  }

  let dirty = $derived(!!saved && !!draft && JSON.stringify(fromDraft(draft)) !== JSON.stringify({ interests: saved.interests, titleBoost: saved.titleBoost, rules: saved.rules }));

  async function saveDraft() {
    if (!saved || !draft) return;
    const next = { ...saved, ...fromDraft(draft) };
    busy = true;
    try {
      await feeds.setSettings(next);
      const open = draft.interests.map((i) => i.open);
      await loadSettings();
      draft!.interests.forEach((i, n) => (i.open = open[n] ?? false));
      notify("Saved", false);
    } catch (err) {
      draftError = String(err);
      notify(draftError, true);
    } finally {
      busy = false;
    }
  }

  /** A setting that saves at once: with the interests and rules as saved. */
  function setNow(change: Partial<FeedSettings>) {
    if (!saved) return;
    const next = { ...saved, ...change };
    act(async () => {
      await feeds.setSettings(next);
      saved = next;
    }, "Saved");
  }

  const number = (e: Event) => Number((e.currentTarget as HTMLInputElement).value);
  const blankLine = (): Line => ({ pattern: false, text: "", weight: 1, label: "" });
</script>

<section id="feeds">
  <h2>Feeds</h2>

  <ul class="subs">
    {#each subscriptions as s (s.id)}
      <li>
        <div class="sub">
          <span class="main">
            <span class="title" title={s.title}>{s.title}</span>
            <span class="hint" title={feedText(s.feed)}>{kindName(s.kind)} · {feedText(s.feed)}{s.boost ? ` · boost ${s.boost > 0 ? "+" : ""}${s.boost}` : ""}</span>
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
                {#each KINDS as k (k.value)}<option value={k.value}>{k.name}</option>{/each}
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
      {#each KINDS as k (k.value)}<option value={k.value}>{k.name}</option>{/each}
    </select>
    <button disabled={busy || !addText.trim()} onclick={add}>Add</button>
  </div>
  <p class="hint">An ISSN is swept in Crossref; an OpenAlex filter needs the OpenAlex key.</p>

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
          {#each list as s (feedKey(s.feed))}
            {@const has = subscribed.has(feedKey(s.feed))}
            <label class="check" class:muted={has}>
              <input type="checkbox" checked={has || picked.has(feedKey(s.feed))} disabled={has} onchange={() => toggle(s)} />
              {s.title}{#if s.kind !== "journal"}<span class="hint inline"> · {kindName(s.kind)}</span>{/if}
            </label>
          {/each}
        </div>
      {/each}
      <button class="primary" disabled={busy || !picked.size} onclick={subscribeSuggested}>
        Subscribe to {picked.size || ""} Selected
      </button>
    </div>
  {/if}

  {#if draft && saved}
    <h3>Interests</h3>
    <p class="hint">Named profiles of weighted terms: an item's score is the weights of the terms it has (twice in the title, by default); it is of the interest from the minimum score on. An exclude vetoes. A pattern is a regular expression.</p>
    {#each draft.interests as interest, i (i)}
      <div class="interest">
        <div class="head">
          <button class="disclosure" onclick={() => (interest.open = !interest.open)}>{interest.open ? "▾" : "▸"}</button>
          <input class="text grow" type="text" placeholder="Name" bind:value={interest.name} />
          <label class="inline-label">Min score <input class="num" type="number" step="0.5" bind:value={interest.min_score} /></label>
          <button class="remove" title="Remove the interest" onclick={() => draft!.interests.splice(i, 1)}>✕</button>
        </div>
        {#if interest.open}
          <div class="lines">
            <span class="hint">Terms</span>
            {#each interest.include as line, n (n)}
              <div class="line">
                <select bind:value={line.pattern}><option value={false}>Term</option><option value={true}>Pattern</option></select>
                <input class="text grow" type="text" spellcheck="false" bind:value={line.text} />
                <input class="num" type="number" step="0.5" title="Weight" bind:value={line.weight} />
                <input class="text label-in" type="text" placeholder="Label" bind:value={line.label} />
                <button class="remove" onclick={() => interest.include.splice(n, 1)}>✕</button>
              </div>
            {/each}
            <button class="small" onclick={() => interest.include.push(blankLine())}>Add Term</button>
            <span class="hint">Excludes</span>
            {#each interest.exclude as line, n (n)}
              <div class="line">
                <select bind:value={line.pattern}><option value={false}>Term</option><option value={true}>Pattern</option></select>
                <input class="text grow" type="text" spellcheck="false" bind:value={line.text} />
                <button class="remove" onclick={() => interest.exclude.splice(n, 1)}>✕</button>
              </div>
            {/each}
            <button class="small" onclick={() => interest.exclude.push(blankLine())}>Add Exclude</button>
          </div>
        {/if}
      </div>
    {/each}
    <div class="actions">
      <button
        class="small"
        onclick={() => draft!.interests.push({ name: "", min_score: 3, include: [blankLine()], exclude: [], open: true })}
        >Add Interest</button
      >
      <label class="inline-label">Title counts <input class="num" type="number" step="0.5" min="1" bind:value={draft.titleBoost} /> ×</label>
    </div>

    <h3>Rules</h3>
    <p class="hint">Applied to the new items of a fetch, the first that matches deciding: a query of the feeds (<code>prof:amoc score:6-</code>, <code>t:editorial</code>) and what to do.</p>
    {#each draft.rules as rule, i (i)}
      <div class="line">
        <input class="text grow" type="text" spellcheck="false" placeholder="Query" bind:value={rule.when} />
        <select bind:value={rule.then}>
          <option value="shortlist">Shortlist</option>
          <option value="seen">Mark seen</option>
          <option value="dismiss">Dismiss</option>
        </select>
        <button class="remove" onclick={() => draft!.rules.splice(i, 1)}>✕</button>
      </div>
    {/each}
    <div class="actions">
      <button class="small" onclick={() => draft!.rules.push({ when: "", then: "shortlist" })}>Add Rule</button>
    </div>

    {#if dirty || draftError}
      <div class="save">
        <span class="hint" class:error={!!draftError}>{draftError || "Interests and rules changed"}</span>
        <span class="buttons">
          <button class="small" onclick={loadSettings}>Revert</button>
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
        <span class="hint">An item no feed lists any more goes after this, unless shortlisted or added</span>
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
  section {
    padding: 4px 14px 12px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--menu-bg);
  }
  h2 {
    margin: 10px 0 4px 2px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  h3 {
    margin: 14px 0 2px;
    font-size: 12px;
    font-weight: 600;
  }
  p.hint {
    margin: 2px 0 6px;
  }
  .hint {
    display: block;
    color: var(--muted);
    font-size: 11.5px;
  }
  .hint.inline {
    display: inline;
  }
  .hint.error {
    color: var(--error);
    -webkit-user-select: text;
    user-select: text;
  }
  .label {
    display: block;
  }
  code {
    font-size: 11px;
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
  .head,
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
  .lines {
    margin: 4px 0 8px 22px;
  }
  .interest {
    padding: 4px 0;
    border-bottom: 1px solid var(--line);
  }
  .inline-label {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
    font-size: 11.5px;
    white-space: nowrap;
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

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 8px 0;
  }
  .row + .row {
    border-top: 1px solid var(--line);
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
  input.label-in {
    width: 80px;
  }
  input.num {
    width: 56px;
  }
  .buttons {
    display: flex;
    flex: none;
    gap: 6px;
    justify-content: flex-end;
  }

  button,
  select {
    font: inherit;
    color: var(--fg);
    padding: 4px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    cursor: pointer;
  }
  select {
    max-width: 120px;
  }
  button.small {
    padding: 2px 8px;
    font-size: 12px;
  }
  button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .remove,
  .disclosure {
    flex: none;
    padding: 0 6px;
    border: none;
    background: none;
    color: var(--muted);
  }
  .remove:hover {
    color: var(--error);
  }

  .switch {
    appearance: none;
    flex: none;
    position: relative;
    width: 34px;
    height: 20px;
    margin: 0;
    border-radius: 10px;
    background: var(--border);
    cursor: pointer;
    transition: background 0.15s;
  }
  .switch::before {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #ffffff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
    transition: transform 0.15s;
  }
  .switch:checked {
    background: var(--accent);
  }
  .switch:checked::before {
    transform: translateX(14px);
  }
</style>
