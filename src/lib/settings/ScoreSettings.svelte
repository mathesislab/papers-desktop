<script lang="ts">
  // The Score tab of the settings window: what the score of an item (0–100) of the Feeds and Search tabs is made of.
  // Its two parts: the keywords, the best of its interests (an interest's minimum score is 50, twice that 100), and
  // the library, how close it is to the papers of the .bib files ticked under *Similar to* (as close as their
  // typical paper is to the others: 100). The score is the higher of both, or one alone; it saves at once, as the
  // .bib files ticked (the app's settings: those unticked, `similar_skip`, so that a .bib added later counts). Then the
  // interests, edited as a draft and saved together with the title boost (the feeds' settings, `FeedPrefs`, which
  // the Feeds tab saves its part of too).
  import type { FeedSettings, Interest, ScoreMode } from "$lib/feeds/api";
  import type { FeedPrefs } from "./feeds.svelte";
  import "./settings.css";
  import { fileName, type Preferences } from "./settings.svelte";

  let { prefs, feedPrefs }: { prefs: Preferences; feedPrefs: FeedPrefs } = $props();
  let settings = $derived(prefs.settings!);
  /** the feeds' settings as saved */
  let saved = $derived(feedPrefs.saved);

  const MODES: { mode: ScoreMode; label: string; hint: string }[] = [
    { mode: "higher", label: "Higher of both", hint: "Without interests, the library alone" },
    { mode: "keywords", label: "Keywords only", hint: "How well it matches your interests" },
    { mode: "library", label: "Library only", hint: "How close it is to the papers of your library" },
  ];

  /** an include or exclude as the editor shows it */
  type Line = { pattern: boolean; text: string; weight: number; label: string };
  type DraftInterest = { name: string; min_score: number; include: Line[]; exclude: Line[]; open: boolean };
  type Draft = { interests: DraftInterest[]; titleBoost: number };

  let draft = $state<Draft | null>(null);
  let draftError = $state("");
  let busy = $state(false);

  // the interests to edit, once the settings are loaded (by the page)
  $effect(() => {
    if (saved && !draft) draft = toDraft(saved);
  });

  /** Save a change of the feeds' settings at once; said at the foot. */
  async function setNow(change: Partial<FeedSettings>) {
    try {
      await feedPrefs.save(change);
      prefs.show("Saved", false);
    } catch (err) {
      prefs.show(String(err), true);
    }
  }

  /** A .bib file ticked under *Similar to*, or unticked: the index of the library made again. */
  function tick(file: string, on: boolean) {
    const skip = settings.similar_skip.filter((f) => f !== file);
    prefs.save({ similar_skip: on ? skip : [...skip, file] });
  }

  // the interests and the title boost: edited as a draft, saved together

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
    };
  }

  function fromDraft(d: Draft): Pick<FeedSettings, "interests" | "titleBoost"> {
    const matcher = (l: Line) => (l.pattern ? { pattern: l.text.trim() } : { term: l.text.trim() });
    const interests: Interest[] = d.interests.map((i) => ({
      name: i.name.trim(),
      include: i.include
        .filter((l) => l.text.trim())
        .map((l) => ({ ...matcher(l), weight: Number(l.weight), ...(l.label.trim() ? { label: l.label.trim() } : {}) })),
      exclude: i.exclude.filter((l) => l.text.trim()).map(matcher),
      min_score: Number(i.min_score),
    }));
    return { interests, titleBoost: Number(d.titleBoost) };
  }

  let dirty = $derived(
    !!saved && !!draft && JSON.stringify(fromDraft(draft)) !== JSON.stringify({ interests: saved.interests, titleBoost: saved.titleBoost }),
  );

  /** The interests and the title boost as saved, those edited dropped; the interests open stay open. */
  function revert() {
    if (!saved) return;
    const open = draft?.interests.map((i) => i.open) ?? [];
    draft = toDraft(saved);
    draft.interests.forEach((i, n) => (i.open = open[n] ?? false));
    draftError = "";
  }

  async function saveDraft() {
    if (!draft) return;
    busy = true;
    try {
      await feedPrefs.save(fromDraft(draft));
      revert();
      prefs.show("Saved", false);
    } catch (err) {
      draftError = String(err);
      prefs.show(draftError, true);
    } finally {
      busy = false;
    }
  }

  const blankLine = (): Line => ({ pattern: false, text: "", weight: 1, label: "" });
</script>

<section>
  <h2>Score</h2>
  <p class="hint">
    The score of an item of the Feeds and Search tabs, 0–100, sorts the Feeds tab and is <code>score:</code> in a
    search or a rule. It has two
    parts, each 0–100, shown as two bars: its keywords, on top, the best of your interests; its library, under, how
    close it is to the papers of your library.
  </p>
  {#if saved}
    <div class="modes" role="radiogroup" aria-label="The score">
      {#each MODES as m (m.mode)}
        <label class="check">
          <input type="radio" name="score" checked={saved.score === m.mode} onchange={() => setNow({ score: m.mode })} />
          <span>{m.label}<span class="hint inline">{` · ${m.hint}`}</span></span>
        </label>
      {/each}
    </div>
  {/if}

  <h3>Similar to</h3>
  <p class="hint">
    The library part: 100 for an item as close to the papers of the files ticked as their typical paper is to the
    others. A .bib added to the library later is ticked.
  </p>
  <ul class="files">
    {#each settings.bibtex as file (file)}
      <li>
        <label class="check file" title={file}>
          <input type="checkbox" checked={!settings.similar_skip.includes(file)} onchange={(e) => tick(file, e.currentTarget.checked)} />
          <span>{fileName(file)}</span>
          <span class="path">{prefs.tilde(file)}</span>
        </label>
      </li>
    {:else}
      <li class="none">No .bib file: add the files of your library in the Library tab.</li>
    {/each}
  </ul>

  {#if draft && saved}
    <h3>Interests</h3>
    <p class="hint">
      Named profiles of weighted terms: an interest's score is the weights of the terms an item has (twice in the
      title, by default); the item is of the interest from the minimum score on, which is 50 of the keywords part, and
      twice it 100. An exclude vetoes: the whole score is 0. A pattern is a regular expression.
    </p>
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

    {#if dirty || draftError}
      <div class="save">
        <span class="hint" class:error={!!draftError}>{draftError || "Interests changed"}</span>
        <span class="buttons">
          <button class="small" onclick={revert}>Revert</button>
          <button class="small primary" disabled={busy || !dirty} onclick={saveDraft}>Save</button>
        </span>
      </div>
    {/if}
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

  .modes {
    margin: 4px 0 6px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 0;
  }
  /* a .bib file: its name, its path after it, muted */
  .check.file {
    min-width: 0;
  }
  .check.file .path {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .line,
  .head,
  .actions,
  .save {
    display: flex;
    align-items: center;
    gap: 6px;
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
    justify-content: flex-end;
  }
  select {
    max-width: 120px;
  }
  button.small {
    padding: 2px 8px;
    font-size: 12px;
  }
  /* the ▸ of an interest, drawn as the ✕ of settings.css */
  .disclosure {
    flex: none;
    padding: 0 6px;
    border: none;
    background: none;
    color: var(--muted);
  }
</style>
