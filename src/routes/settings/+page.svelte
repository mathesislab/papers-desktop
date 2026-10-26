<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import "$lib/theme.css";
  import { mac, matches } from "$lib/keys";

  // Mirrors src-tauri/src/settings.rs and the SettingsView of lib.rs
  type Settings = {
    bibtex: string[];
    hotkey: string;
    matching: "word-start" | "inside-words";
    citation_style: string;
  };
  type View = { settings: Settings; styles: string[]; start_at_login: boolean; home: string | null };

  /** the option of the style menu that asks for a .csl file */
  const OTHER = "\u0000other";

  let settings = $state<Settings | null>(null);
  let styles = $state<string[]>([]);
  let startAtLogin = $state(false);
  let home = $state<string | null>(null);
  let notice = $state<{ text: string; error: boolean } | null>(null);
  let recording = $state(false);
  let timer: ReturnType<typeof setTimeout>;

  // a style that is not bundled: a .csl file
  let custom = $derived(
    settings && !styles.includes(settings.citation_style) ? settings.citation_style : null,
  );

  onMount(refresh);

  async function refresh() {
    const view = await invoke<View>("get_settings");
    settings = view.settings;
    styles = view.styles;
    startAtLogin = view.start_at_login;
    home = view.home;
  }

  /** Save a change, which applies at once; on failure, show the settings as they are. */
  async function save(change: Partial<Settings>) {
    if (!settings) return;
    const changed = { ...settings, ...change };
    try {
      await invoke("save_settings", { settings: changed });
      settings = changed;
      show("Saved", false);
    } catch (err) {
      show(String(err), true);
      await refresh();
    }
  }

  function show(text: string, error: boolean) {
    notice = { text, error };
    clearTimeout(timer);
    if (!error) timer = setTimeout(() => (notice = null), 1500);
  }

  async function addBibs() {
    const picked = await open({
      multiple: true,
      title: "Add .bib files",
      filters: [{ name: "BibTeX", extensions: ["bib"] }],
    });
    if (!picked || !settings) return;
    const added = (Array.isArray(picked) ? picked : [picked]).filter(
      (file) => !settings!.bibtex.includes(file),
    );
    if (added.length) await save({ bibtex: [...settings.bibtex, ...added] });
  }

  async function chooseStyle(e: Event & { currentTarget: HTMLSelectElement }) {
    const value = e.currentTarget.value;
    if (value !== OTHER) return save({ citation_style: value });
    // the menu shows the current style until a file is chosen
    e.currentTarget.value = settings!.citation_style;
    const file = await open({ title: "A CSL style", filters: [{ name: "CSL style", extensions: ["csl"] }] });
    if (typeof file === "string") await save({ citation_style: file });
  }

  async function toggleStartAtLogin() {
    try {
      startAtLogin = await invoke<boolean>("set_start_at_login", { on: !startAtLogin });
    } catch (err) {
      show(String(err), true);
    }
  }

  /** While recording, the keys pressed become the hotkey; Esc gives up. */
  function onkeydown(e: KeyboardEvent) {
    if (!recording) {
      if (matches(e, { key: "w", mod: true })) getCurrentWindow().close();
      return;
    }
    e.preventDefault();
    if (e.key === "Escape") {
      recording = false;
      return;
    }
    const key = keyName(e.code);
    if (!key) return; // a modifier, so far
    const modifiers = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Cmd"];
    const held = modifiers.filter((m): m is string => !!m);
    if (!held.length) {
      show(`A hotkey needs a modifier: ${mac ? "⌘, ⌥, ⌃ or ⇧" : "Ctrl, Alt or Shift"}`, true);
      return;
    }
    recording = false;
    save({ hotkey: [...held, key].join("+") });
  }

  /** The name of a key for the hotkey ("KeyP" is P), or none for a modifier. */
  function keyName(code: string): string | null {
    if (/^(Meta|OS|Control|Alt|Shift)(Left|Right)?$|^(Fn|CapsLock)$/.test(code)) return null;
    return code.replace(/^Key/, "").replace(/^Digit/, "");
  }

  /** "Alt+Space" as ⌥Space on macOS. */
  function pretty(hotkey: string): string {
    if (!mac) return hotkey;
    const symbols: Record<string, string> = { Ctrl: "⌃", Control: "⌃", Alt: "⌥", Option: "⌥", Shift: "⇧", Cmd: "⌘", Command: "⌘", Super: "⌘" };
    const parts = hotkey.split("+");
    const key = parts.pop()!;
    return parts.map((part) => symbols[part] ?? part).join("") + key;
  }

  const styleName = (name: string) =>
    name.split("-").map((word) => word[0].toUpperCase() + word.slice(1)).join(" ");
  const fileName = (path: string) => path.split(/[\\/]/).pop()!;

  /** The folder of a file, with ~ for the home folder. */
  function folder(path: string): string {
    const dir = path.slice(0, path.length - fileName(path).length - 1);
    return home && dir.startsWith(home) ? "~" + dir.slice(home.length) : dir;
  }
</script>

<svelte:window {onkeydown} />

{#if settings}
  <main>
    <section>
      <h2>Library</h2>
      <ul class="files">
        {#each settings.bibtex as file (file)}
          <li>
            <span class="file" title={file}>
              <span>{fileName(file)}</span>
              <span class="path">{folder(file)}</span>
            </span>
            <button
              class="remove"
              title="Remove from the library"
              onclick={() => save({ bibtex: settings!.bibtex.filter((f) => f !== file) })}>✕</button
            >
          </li>
        {:else}
          <li class="none">No .bib file: add the files of your library.</li>
        {/each}
      </ul>
      <button onclick={addBibs}>Add .bib Files…</button>
      <p class="hint">The panel searches them all, and reloads one when it changes.</p>
    </section>

    <section>
      <h2>Search</h2>
      <div class="row">
        <span>
          <span class="label">Hotkey</span>
          <span class="hint">Shows the panel, from any app</span>
        </span>
        <button class="hotkey" class:recording onclick={() => (recording = !recording)}>
          {recording ? "Press the keys…" : pretty(settings.hotkey)}
        </button>
      </div>
      <label class="row">
        <span>
          <span class="label">Match inside words</span>
          <span class="hint">“stab” also finds Multistability</span>
        </span>
        <input
          type="checkbox"
          class="switch"
          checked={settings.matching === "inside-words"}
          onchange={(e) => save({ matching: e.currentTarget.checked ? "inside-words" : "word-start" })}
        />
      </label>
    </section>

    <section>
      <h2>Citations</h2>
      <label class="row">
        <span>
          <span class="label">Style</span>
          <span class="hint">Of the citation in the preview</span>
        </span>
        <select value={settings.citation_style} onchange={chooseStyle}>
          {#each styles as name (name)}<option value={name}>{styleName(name)}</option>{/each}
          {#if custom}<option value={custom}>{fileName(custom)}</option>{/if}
          <option value={OTHER}>Other .csl File…</option>
        </select>
      </label>
    </section>

    <section>
      <h2>General</h2>
      <label class="row">
        <span class="label">Start at login</span>
        <input type="checkbox" class="switch" checked={startAtLogin} onchange={toggleStartAtLogin} />
      </label>
    </section>

    <footer class:error={notice?.error}>{notice?.text ?? "Changes apply at once."}</footer>
  </main>
{/if}

<style>
  :global(html, body) {
    margin: 0;
    background: var(--bg);
    color: var(--fg);
    font-size: 13px;
    cursor: default;
    -webkit-user-select: none;
    user-select: none;
  }

  main {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 20px 22px 16px;
  }

  h2 {
    margin: 0 0 6px 2px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  section {
    padding: 4px 14px 12px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--menu-bg);
  }
  section h2 {
    margin: 10px 0 4px;
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
  .label {
    display: block;
  }
  .hint {
    display: block;
    margin-top: 1px;
    color: var(--muted);
    font-size: 11.5px;
  }

  .files {
    margin: 4px 0 10px;
    padding: 0;
    list-style: none;
  }
  .files li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 0;
    border-bottom: 1px solid var(--line);
  }
  .files .file {
    flex: 1;
    min-width: 0;
  }
  .files .file > span {
    display: block;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .files .path {
    color: var(--muted);
    font-size: 11px;
  }
  .files .none {
    color: var(--muted);
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
    max-width: 230px;
  }
  .remove {
    padding: 0 6px;
    border: none;
    background: none;
    color: var(--muted);
  }
  .remove:hover {
    color: var(--error);
  }
  .hotkey {
    min-width: 120px;
    font-variant-numeric: tabular-nums;
  }
  .hotkey.recording {
    border-color: var(--accent);
    color: var(--accent);
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

  footer {
    color: var(--muted);
    font-size: 11.5px;
    text-align: center;
  }
  footer.error {
    color: var(--error);
    -webkit-user-select: text;
    user-select: text;
  }
</style>
