<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, tick } from "svelte";
  import "$lib/theme.css";
  import { label, mac, matches, type Shortcut } from "$lib/keys";
  import { resetStyle } from "$lib/cite";
  import Preview from "$lib/Preview.svelte";
  import Marked from "$lib/Marked.svelte";
  import { clearYears, extendYears, onlyYears, toggleExcluded, toggleYear } from "$lib/query";
  import YearStrip from "$lib/YearStrip.svelte";
  import type { Found, Hit, Item, Status, YearCount } from "$lib/types";

  type Action = {
    /** in the list of shortcuts, and the label unless `label` is given */
    name: string;
    /** the label for one item, in the action menu and the footer */
    label?: (it: Item) => string;
    shortcut: Shortcut;
    when?: (it: Item) => boolean;
    run: (it: Item) => Promise<void>;
  };

  let query = $state("");
  let hits = $state<Hit[]>([]);
  let total = $state(0);
  let years = $state<YearCount[]>([]);
  let selected = $state(0);
  let status = $state<Status>({ state: "loading" });
  let notice = $state<{ text: string; error: boolean } | null>(null);
  let menu = $state(false);
  let menuSelected = $state(0);
  let preview = $state(storedPreview());
  let input: HTMLInputElement;
  let list = $state<HTMLUListElement>();

  const call = (command: string) => async (it: Item) => {
    await invoke(command, { id: it.id });
  };

  const copy = (format: string) => async (it: Item) => {
    const text = await invoke<string>("copy", { id: it.id, format });
    notice = { text: `Copied ${text.split("\n")[0]}`, error: false };
    setTimeout(() => invoke("hide_panel"), 400);
  };

  const hasLink = (it: Item) => !!(it.doi || it.url);

  const actions: Action[] = [
    {
      name: "Open PDF, else DOI page",
      label: (it) => (it.files.length ? "Open PDF" : "Open DOI page"),
      shortcut: { key: "Enter" },
      when: (it) => it.files.length > 0 || hasLink(it),
      run: call("open"),
    },
    {
      name: mac ? "Reveal in Finder" : "Show in folder",
      shortcut: { key: "Enter", mod: true },
      when: (it) => it.files.length > 0,
      run: call("reveal"),
    },
    { name: "Open DOI page", shortcut: { key: "d", mod: true }, when: hasLink, run: call("open_link") },
    { name: "Copy key", shortcut: { key: "c", mod: true, shift: true }, run: copy("key") },
    { name: "Copy [@key]", shortcut: { key: ".", mod: true }, run: copy("pandoc") },
    { name: "Copy \\cite{key}", shortcut: { key: "l", mod: true }, run: copy("latex") },
    { name: "Copy BibTeX", shortcut: { key: "b", mod: true }, run: copy("bibtex") },
  ];
  const menuShortcut: Shortcut = { key: "k", mod: true };
  const previewShortcut: Shortcut = { key: "p", mod: true };
  const settingsShortcut: Shortcut = { key: ",", mod: true };

  /** the list shown in the empty panel */
  const shortcuts: [string, string][] = [
    ...actions.map((a): [string, string] => [a.name, label(a.shortcut)]),
    ["Preview", label(previewShortcut)],
    ["Settings", label(settingsShortcut)],
    ["All actions", label(menuShortcut)],
    ["Move", "↑ ↓"],
    ["Close", "Esc"],
  ];

  /** the field filters of the query, also in the empty panel */
  const filters = [
    ["a:", "author"],
    ["f:", "first author"],
    ["t:", "title"],
    ["y:", "year, 2010-2015"],
    ["j:", "journal"],
    ["k:", "key"],
    ["doi:", ""],
    ["tag:", ""],
    ['"…"', "phrase"],
    ["-", "exclude"],
  ];

  const actionLabel = (a: Action, it: Item) => a.label?.(it) ?? a.name;

  // the preview stays on or off between sessions
  function storedPreview(): boolean {
    try {
      return localStorage.getItem("preview") === "on";
    } catch {
      return false;
    }
  }

  function setPreview(on: boolean) {
    preview = on;
    invoke("set_preview", { on });
    try {
      localStorage.setItem("preview", on ? "on" : "off");
    } catch {}
  }

  let current = $derived(hits[selected]?.item);
  let available = $derived(current ? actions.filter((a) => !a.when || a.when(current)) : []);

  async function run(action: Action) {
    const item = current;
    menu = false;
    if (!item || (action.when && !action.when(item))) return;
    try {
      await action.run(item);
    } catch (err) {
      notice = { text: String(err), error: true };
    }
  }

  async function search() {
    const q = query;
    const found = await invoke<Found>("search", { query: q });
    if (q !== query) return; // a newer search is on its way
    hits = found.hits;
    total = found.total;
    years = found.years;
    selected = 0;
    menu = false;
    notice = null;
    list?.scrollTo(0, 0);
  }

  /** A query edited by a click: search it, and give the field back its focus. */
  function edit(edited: string) {
    query = edited;
    search();
    input.focus();
  }

  /** A click on a year: only it; with ⇧, extend the range to it; with ⌥, exclude it. */
  function pickYear(year: number, e: MouseEvent) {
    if (e.altKey) edit(toggleExcluded(query, year));
    else if (e.shiftKey) edit(extendYears(query, year));
    else edit(toggleYear(query, year));
  }

  async function select(i: number) {
    selected = Math.max(0, Math.min(hits.length - 1, i));
    await tick();
    list?.children[selected]?.scrollIntoView({ block: "nearest" });
  }

  function move(e: KeyboardEvent): boolean {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const step = e.key === "ArrowDown" ? 1 : -1;
      if (menu) menuSelected = (menuSelected + step + available.length) % available.length;
      else select(selected + step);
      return true;
    }
    return false;
  }

  function onkeydown(e: KeyboardEvent) {
    if (move(e)) {
      e.preventDefault();
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (menu) menu = false;
      else invoke("hide_panel");
    } else if (matches(e, menuShortcut)) {
      e.preventDefault();
      menu = !menu && available.length > 0;
      menuSelected = 0;
    } else if (matches(e, previewShortcut)) {
      e.preventDefault();
      setPreview(!preview);
    } else if (matches(e, settingsShortcut)) {
      e.preventDefault();
      invoke("open_settings");
    } else if (menu && e.key === "Enter") {
      e.preventDefault();
      run(available[menuSelected]);
    } else {
      const action = actions.find((a) => matches(e, a.shortcut));
      if (action) {
        e.preventDefault();
        run(action);
      }
    }
  }

  onMount(() => {
    invoke<Status>("status").then((s) => (status = s));
    if (preview) invoke("set_preview", { on: true });
    const unlisten = [
      listen<Status>("status", (e) => {
        status = e.payload;
        if (status.state === "ready") {
          // the settings or the entries may have changed
          resetStyle();
          search();
        }
      }),
      listen("shown", () => {
        menu = false;
        notice = null;
        input.focus();
        input.select();
      }),
    ];
    return () => unlisten.forEach((u) => u.then((f) => f()));
  });
</script>

<svelte:window {onkeydown} />

<main class="panel">
  <input
    bind:this={input}
    bind:value={query}
    oninput={search}
    placeholder="Search papers"
    spellcheck="false"
    autocomplete="off"
  />

  <div class="body">
    <div class="results">
      {#if years.length > 1}
        <YearStrip
          {years}
          onyear={pickYear}
          onrange={(from, to) => edit(onlyYears(query, from, to))}
          onclear={() => edit(clearYears(query))}
        />
      {/if}
      {#if hits.length}
        <ul bind:this={list} role="listbox">
          {#each hits as hit, i (hit.item.id)}
            <!-- the keys are handled on the window -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <li
              role="option"
              aria-selected={i === selected}
              tabindex="-1"
              class:selected={i === selected}
              onclick={() => select(i)}
              ondblclick={() => run(actions[0])}
            >
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <span
                class="year"
                class:repeat={i > 0 && hits[i - 1].item.year === hit.item.year}
                title="Only this year · ⇧ extend the range · ⌥ exclude it"
                onmousedown={(e) => e.preventDefault()}
                onclick={(e) => {
                  if (hit.item.year === null) return;
                  e.stopPropagation();
                  pickYear(hit.item.year, e);
                }}>{hit.item.year ?? ""}</span
              >
              <div class="text">
                <div class="title"><Marked spans={hit.title} /></div>
                <div class="subtitle"><Marked spans={hit.subtitle} /></div>
              </div>
              {#if hit.item.files.length}
                <span class="badge">PDF</span>
              {:else if hit.item.doi}
                <span class="badge">DOI</span>
              {/if}
            </li>
          {/each}
        </ul>
      {:else}
        <div class="empty">
          {#if !query.trim()}
            <dl class="shortcuts">
              {#each shortcuts as [name, keys]}
                <dt>{name}</dt>
                <dd><kbd>{keys}</kbd></dd>
              {/each}
            </dl>
            <p class="filters">
              {#each filters as [prefix, field], i}{#if i} · {/if}<kbd>{prefix}</kbd>&nbsp;{field}{/each}
            </p>
          {:else if status.state === "ready"}
            No match
          {/if}
        </div>
      {/if}

      {#if menu && current}
        <div class="menu" role="menu">
          {#each available as action, i}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              role="menuitem"
              tabindex="-1"
              class:selected={i === menuSelected}
              onclick={() => run(action)}
              onmouseenter={() => (menuSelected = i)}
            >
              <span>{actionLabel(action, current)}</span>
              <kbd>{label(action.shortcut)}</kbd>
            </div>
          {/each}
        </div>
      {/if}

      <footer class:error={notice?.error || (!notice && status.state === "failed")}>
        <span class="status">
          {#if notice}
            {notice.text}
          {:else if status.state === "loading"}
            Loading library…
          {:else if status.state === "ready" && query.trim()}
            {total === hits.length ? total : `Newest ${hits.length} of ${total}`}
            {total === 1 ? "match" : "matches"}
          {:else if status.state === "ready"}
            {status.count} papers
          {:else}
            {status.error}
          {/if}
        </span>
        {#if current && !notice}
          <span class="hints">
            {actionLabel(actions[0], current)} <kbd>{label(actions[0].shortcut)}</kbd>
            Preview <kbd>{label(previewShortcut)}</kbd>
            Actions <kbd>{label(menuShortcut)}</kbd>
          </span>
        {/if}
      </footer>
    </div>
    {#if preview && current}
      <Preview item={current} marked={hits[selected].authors} onopen={() => run(actions[0])} />
    {/if}
  </div>
</main>

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    overflow: hidden;
    background: transparent;
    color: var(--fg);
    -webkit-user-select: none;
    user-select: none;
    cursor: default;
  }

  .panel {
    box-sizing: border-box;
    height: 100vh;
    display: flex;
    flex-direction: column;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: hidden;
  }

  input {
    flex: none;
    padding: 14px 18px;
    border: none;
    border-bottom: 1px solid var(--line);
    outline: none;
    background: transparent;
    color: var(--fg);
    font: inherit;
    font-size: 20px;
  }
  input::placeholder {
    color: var(--muted);
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  /* the list, its action menu and footer keep their place when the preview opens */
  .results {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  ul {
    flex: 1;
    min-width: 0;
    margin: 0;
    padding: 6px;
    list-style: none;
    overflow-y: auto;
  }
  li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 12px;
    border-radius: 7px;
  }
  li.selected,
  .menu .selected {
    background: var(--accent);
    color: var(--on-accent);
  }
  .year {
    flex: none;
    width: 38px;
    font-size: 14px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }
  .year.repeat {
    opacity: 0.3;
  }
  .year:hover {
    text-decoration: underline;
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  .title,
  .subtitle {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .title {
    font-size: 14px;
    font-weight: 500;
  }
  .subtitle {
    margin-top: 2px;
    font-size: 12px;
    color: var(--muted);
    /* matched words stand out of the muted line */
    --mark: var(--fg);
  }
  li.selected .subtitle {
    color: inherit;
    opacity: 0.85;
    --mark: inherit;
  }
  .badge {
    flex: none;
    padding: 1px 6px;
    border: 1px solid currentColor;
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    opacity: 0.55;
  }

  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    color: var(--muted);
    font-size: 14px;
  }

  .shortcuts {
    display: grid;
    grid-template-columns: auto auto;
    gap: 5px 28px;
    margin: 0;
    font-size: 13px;
  }
  .shortcuts dt {
    text-align: right;
  }
  .shortcuts dd {
    margin: 0;
  }
  .shortcuts kbd {
    font-size: 13px;
  }
  .filters {
    max-width: 520px;
    margin: 18px 0 0;
    font-size: 12px;
    line-height: 1.6;
    text-align: center;
  }
  .filters kbd {
    font-size: 12px;
    opacity: 1;
    color: var(--fg);
  }

  .menu {
    position: absolute;
    right: 10px;
    bottom: 36px;
    min-width: 240px;
    padding: 5px;
    background: var(--menu-bg);
    border: 1px solid var(--border);
    border-radius: 9px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.22);
    font-size: 13px;
  }
  .menu div {
    display: flex;
    justify-content: space-between;
    gap: 24px;
    padding: 6px 10px;
    border-radius: 6px;
  }

  kbd {
    font: inherit;
    font-size: 11px;
    opacity: 0.7;
  }

  footer {
    flex: none;
    display: flex;
    justify-content: space-between;
    gap: 16px;
    padding: 6px 18px;
    border-top: 1px solid var(--line);
    color: var(--muted);
    font-size: 11px;
  }
  footer .status {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    -webkit-user-select: text;
    user-select: text;
  }
  footer .hints {
    flex: none;
  }
  footer .hints kbd {
    margin: 0 8px 0 3px;
  }
  footer .hints kbd:last-child {
    margin-right: 0;
  }
  footer.error .status {
    color: var(--error);
    white-space: normal;
  }
</style>
