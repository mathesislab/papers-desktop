<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount, tick } from "svelte";
  import "$lib/theme.css";
  import { mac, matches, zoomStep } from "$lib/keys";
  import { writers as listWriters } from "$lib/library/writes";
  import type { Writer } from "$lib/types";

  // Mirrors src-tauri/src/settings.rs and the SettingsView of lib.rs
  type Settings = {
    bibtex: string[];
    hotkey: string;
    /** on Linux only */
    selection_hotkey?: string;
    matching: "word-start" | "inside-words";
    citation_style: string;
    check_updates: boolean;
    after_close: "library" | "next";
    /** the zoom of the windows: the text size */
    text_size: "smallest" | "smaller" | "default" | "larger" | "largest";
    /** what writes the papers libraries: papers-rs, built in, or a papers command */
    writer: "built-in" | "command";
    /** the papers command, for the writer "command"; empty: the one the login shell finds */
    papers: string;
    /** sent to doi.org and OpenAlex, which answer faster to programs that give one; empty: nothing is sent */
    email: string;
    /** a free key of OpenAlex, for more searches a day; empty: none. Sent to OpenAlex only */
    openalex_key: string;
    /** the tags pinned to the keys 1–9, set from the sidebar of the main window: kept as they are */
    pinned_tags: (string | null)[];
  };
  /** mirrors cli_tool::Status */
  type Cli = {
    available: boolean;
    link: string | null;
    state: "missing" | "ours" | "foreign";
    points_to: string | null;
    current: boolean;
  };
  /** mirrors cli_tool::Failure */
  type CliFailure = { kind: "foreign" | "denied" | "failed"; message: string };
  /** mirrors cli_tool::PathAdvice: `line` to add to the startup file `file` of the shell; `added`: it is there already */
  type CliPath = { on_path: boolean; line: string; file: string | null; added: boolean };
  /** mirrors update::Available */
  type Update = { version: string };
  /** mirrors update::Installed */
  type Installed = { version: string; from: string };
  type View = { settings: Settings; styles: string[]; start_at_login: boolean; home: string | null };

  /** the option of the style menu that asks for a .csl file */
  const OTHER = "\u0000other";

  let settings = $state<Settings | null>(null);
  let styles = $state<string[]>([]);
  let startAtLogin = $state(false);
  let home = $state<string | null>(null);
  let notice = $state<{ text: string; error: boolean } | null>(null);
  /** the hotkey whose keys are being recorded */
  type HotkeyField = "hotkey" | "selection_hotkey";
  let recording = $state<HotkeyField | null>(null);
  let timer: ReturnType<typeof setTimeout>;

  // a style that is not bundled: a .csl file
  let custom = $derived(
    settings && !styles.includes(settings.citation_style) ? settings.citation_style : null,
  );

  // updates: the version found, and where its install is; the one installed
  // just before this start
  let version = $state("");
  let update = $state<Update | null>(null);
  let installed = $state<Installed | null>(null);
  let updating = $state<"idle" | "checking" | "current" | "installing" | "failed">("idle");
  let updateError = $state("");
  let progress = $state<{ done: number; total: number | null }>({ done: 0, total: null });

  let updateStatus = $derived.by(() => {
    switch (updating) {
      case "checking":
        return "Looking for updates…";
      case "current":
        return "Papers Desktop is up to date.";
      case "failed":
        return updateError;
      case "installing":
        return progress.total
          ? `Downloading… ${Math.round((100 * progress.done) / progress.total)}%`
          : "Downloading…";
      default:
        return update ? `Version ${update.version} is available.` : "";
    }
  });

  onMount(() => {
    load();
    const unlisten = [
      listen<Update | null>("update", (e) => (update = e.payload)),
      listen<[number, number | null]>("update-progress", (e) => {
        progress = { done: e.payload[0], total: e.payload[1] };
      }),
      listen<string>("section", (e) => showSection(e.payload)),
      // ⌘+ and ⌘- in another window
      listen<Settings["text_size"]>("text-size", (e) => {
        if (settings) settings.text_size = e.payload;
      }),
    ];
    return () => unlisten.forEach((u) => u.then((f) => f()));
  });

  /** Fill the window, then show it: it is made hidden, not to flash empty.
   *  Its URL may name a section to show ("settings#updates"). */
  async function load() {
    await Promise.allSettled([
      refresh(),
      getVersion().then((v) => (version = v)),
      invoke<Update | null>("found_update").then((found) => (update = found)),
      invoke<Installed | null>("installed_update").then((done) => (installed = done)),
    ]);
    await tick();
    const window = getCurrentWindow();
    await window.show();
    await window.setFocus();
    if (location.hash) showSection(location.hash.slice(1));
  }

  /** Scroll to a section; the updates also look for one, unless found. */
  function showSection(id: string) {
    document.getElementById(id)?.scrollIntoView({ block: "start" });
    if (id === "updates" && !update && updating !== "checking" && updating !== "installing") checkUpdate();
  }

  async function checkUpdate() {
    updating = "checking";
    try {
      update = await invoke<Update | null>("check_update");
      updating = update ? "idle" : "current";
    } catch (err) {
      updateError = `Cannot look for updates: ${err}`;
      updating = "failed";
    }
  }

  /** Download and install the update; the app then restarts. */
  async function installUpdate() {
    updating = "installing";
    try {
      await invoke("install_update");
    } catch (err) {
      updateError = `Cannot install the update: ${err}`;
      updating = "failed";
    }
  }

  async function refresh() {
    const view = await invoke<View>("get_settings");
    settings = view.settings;
    styles = view.styles;
    startAtLogin = view.start_at_login;
    home = view.home;
    writers = await listWriters();
    cli = await invoke<Cli>("cli_status");
    // starts the shell of the user: not waited for
    findCliPath();
  }

  /** the papers-rs command line tool: its link, and whether ~/.local/bin is in the PATH of the user's shell */
  let cli = $state<Cli | null>(null);
  let cliBusy = $state(false);
  let cliPath = $state<CliPath | null>(null);

  async function findCliPath() {
    cliPath = null;
    if (cli?.state !== "ours") return;
    try {
      cliPath = await invoke<CliPath>("cli_path");
    } catch {
      cliPath = null;
    }
  }

  /** Add ~/.local/bin to the PATH in the startup file of the shell, on the click. */
  async function addCliPath() {
    cliBusy = true;
    try {
      cliPath = await invoke<CliPath>("add_cli_path");
      show(`Added to ${tilde(cliPath.file ?? "")}`, false);
    } catch (err) {
      show((err as CliFailure).message ?? String(err), true);
    } finally {
      cliBusy = false;
    }
  }

  let cliStatus = $derived.by(() => {
    if (!cli) return "";
    if (!cli.available) return "Not in this build of the app";
    const link = cli.link ? tilde(cli.link) : "";
    if (cli.state === "foreign") return `${link} is another program: left as it is`;
    if (cli.state === "missing") return "Not installed";
    return cli.current ? `Installed: ${link}` : `${link} points to another copy: ${tilde(cli.points_to ?? "")}`;
  });

  async function changeCli(command: "install_cli" | "uninstall_cli") {
    cliBusy = true;
    try {
      cli = await invoke<Cli>(command);
      show(command === "install_cli" ? "Installed" : "Removed", false);
      findCliPath();
    } catch (err) {
      show((err as CliFailure).message ?? String(err), true);
    } finally {
      cliBusy = false;
    }
  }

  /** A path with ~ for the home folder. */
  function tilde(path: string): string {
    return home && path.startsWith(home + "/") ? "~" + path.slice(home.length) : path;
  }

  /** how each .bib file is written: by papers-rs, by a papers command, or by the app */
  let writers = $state<Writer[]>([]);

  function writtenBy(file: string): Writer | undefined {
    const path = home && file.startsWith("~/") ? home + file.slice(1) : file;
    return writers.find((w) => w.bib === path);
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

  let recentCleared = $state(false);

  async function clearRecent() {
    await invoke("clear_recent");
    recentCleared = true;
  }

  async function releaseNotes(of: string) {
    try {
      await invoke("release_notes", { version: of });
    } catch (err) {
      show(String(err), true);
    }
  }

  async function sendFeedback() {
    try {
      await invoke("send_feedback");
    } catch (err) {
      show(String(err), true);
    }
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
      const zoom = zoomStep(e);
      if (zoom !== null) {
        e.preventDefault();
        invoke("text_size_step", { steps: zoom });
      }
      return;
    }
    e.preventDefault();
    if (e.key === "Escape") {
      recording = null;
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
    const field = recording;
    recording = null;
    save({ [field]: [...held, key].join("+") });
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

{#snippet recorder(field: HotkeyField, keys: string)}
  <button
    class="hotkey"
    class:recording={recording === field}
    onclick={() => (recording = recording === field ? null : field)}
  >
    {recording === field ? "Press the keys…" : pretty(keys)}
  </button>
{/snippet}

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
              {#if writtenBy(file)}
                {@const writer = writtenBy(file)!}
                <span class="path" class:error={writer.kind === "papers-missing" || writer.problem !== null}>
                  {writer.problem ??
                    (writer.kind === "papers-rs"
                      ? "A papers library, written by papers-rs (built in)"
                      : writer.kind === "papers"
                        ? `A papers library, written by papers ${writer.version}`
                        : writer.kind === "native"
                          ? "Written by the app"
                          : "A papers library: read-only until the papers command (3.0.0b4 or later) is found")}
                </span>
              {/if}
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
      <label class="row">
        <span>
          <span class="label">Writes papers libraries</span>
          <span class="hint">Built in, or a command of the papers standard; the files and the history are the same</span>
        </span>
        <select
          value={settings.writer}
          onchange={(e) => save({ writer: e.currentTarget.value as Settings["writer"] })}
        >
          <option value="built-in">Built in (papers-rs)</option>
          <option value="command">A papers command</option>
        </select>
      </label>
      {#if settings.writer === "command"}
        <label class="row">
          <span>
            <span class="label">papers command</span>
            <span class="hint">Its path; empty: the one your shell finds</span>
          </span>
          <input
            class="text"
            type="text"
            placeholder="papers"
            spellcheck="false"
            value={settings.papers}
            onchange={(e) => save({ papers: e.currentTarget.value.trim() })}
          />
        </label>
      {/if}
    </section>

    <section>
      <h2>Search</h2>
      <div class="row">
        <span>
          <span class="label">Hotkey</span>
          <span class="hint">Shows the panel, from any app</span>
        </span>
        {@render recorder("hotkey", settings.hotkey)}
      </div>
      {#if settings.selection_hotkey !== undefined}
        <div class="row">
          <span>
            <span class="label">Search the selection</span>
            <span class="hint">Finds the text selected in any app</span>
          </span>
          {@render recorder("selection_hotkey", settings.selection_hotkey)}
        </div>
      {:else if mac}
        <div class="row">
          <span>
            <span class="label">Search the selection</span>
            <span class="hint"
              >In any app: Services → Search Papers. System Settings → Keyboard → Keyboard Shortcuts → Services
              gives it a shortcut.</span
            >
          </span>
        </div>
      {/if}
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
      <label class="row">
        <span>
          <span class="label">Text size</span>
          <span class="hint">Zooms every window ({mac ? "⌘+ ⌘- ⌘0" : "Ctrl+ + - 0"})</span>
        </span>
        <select
          value={settings.text_size}
          onchange={(e) => save({ text_size: e.currentTarget.value as Settings["text_size"] })}
        >
          <option value="smallest">Smallest</option>
          <option value="smaller">Smaller</option>
          <option value="default">Default</option>
          <option value="larger">Larger</option>
          <option value="largest">Largest</option>
        </select>
      </label>
      <div class="row">
        <span>
          <span class="label">Recent papers</span>
          <span class="hint">The papers opened or copied last, listed in the empty panel</span>
        </span>
        <button disabled={recentCleared} onclick={clearRecent}>{recentCleared ? "Cleared" : "Clear"}</button>
      </div>
      <label class="row">
        <span>
          <span class="label">After closing a paper</span>
          <span class="hint">In the main window ({mac ? "⌘W" : "Ctrl+W"})</span>
        </span>
        <select
          value={settings.after_close}
          onchange={(e) => save({ after_close: e.currentTarget.value as Settings["after_close"] })}
        >
          <option value="next">Show the next paper open</option>
          <option value="library">Show the library</option>
        </select>
      </label>
      <div class="row">
        <span>
          <span class="label">Command line tool</span>
          <span class="hint" class:error={cli?.state === "foreign"}>{cliStatus}</span>
          <span class="hint"
            >papers-rs is the same program as the built-in writer; papers (Python) can be installed alongside.</span
          >
          {#if cliPath && !cliPath.on_path}
            <span class="hint">~/.local/bin is not in the PATH of your shell. Add this line to its startup file:</span>
            <code class="cmd">{cliPath.line}</code>
            {#if cliPath.added}
              <span class="hint">It is in {tilde(cliPath.file ?? "")} already: open a new terminal.</span>
            {/if}
          {/if}
        </span>
        {#if cli?.available && cli.state !== "foreign"}
          <span class="buttons">
            {#if cliPath && !cliPath.on_path && cliPath.file && !cliPath.added}
              <button disabled={cliBusy} onclick={addCliPath}>Add to {tilde(cliPath.file)}</button>
            {/if}
            {#if cli.state === "ours"}
              <button disabled={cliBusy} onclick={() => changeCli("uninstall_cli")}>Uninstall</button>
            {/if}
            {#if cli.state === "missing" || !cli.current}
              <button disabled={cliBusy} onclick={() => changeCli("install_cli")}>
                {cli.state === "missing" ? "Install" : "Point to This App"}
              </button>
            {/if}
          </span>
        {/if}
      </div>
      <label class="row">
        <span>
          <span class="label">Email</span>
          <span class="hint">Sent to doi.org and OpenAlex, which answer faster to programs that give one (optional).</span>
        </span>
        <input
          class="text"
          type="email"
          placeholder="you@example.org"
          spellcheck="false"
          autocapitalize="off"
          value={settings.email}
          onchange={(e) => save({ email: e.currentTarget.value.trim() })}
        />
      </label>
      <label class="row">
        <span>
          <span class="label">OpenAlex API key</span>
          <span class="hint">Free key from openalex.org: more searches a day than without (optional)</span>
        </span>
        <input
          class="text"
          type="password"
          autocomplete="off"
          spellcheck="false"
          value={settings.openalex_key}
          onchange={(e) => save({ openalex_key: e.currentTarget.value.trim() })}
        />
      </label>
    </section>

    <section id="updates">
      <h2>Updates</h2>
      <div class="row">
        <span>
          <span class="label">Version {version}</span>
          <span class="hint" class:error={updating === "failed"}>{updateStatus}</span>
        </span>
        {#if update}
          <button class="primary" disabled={updating === "installing"} onclick={installUpdate}>
            Install {update.version} and Restart
          </button>
        {:else}
          <button disabled={updating === "checking"} onclick={checkUpdate}>Check for Updates Now</button>
        {/if}
      </div>
      {#if updating === "installing"}
        <progress value={progress.done} max={progress.total ?? undefined}></progress>
      {/if}
      {#if installed}
        {@const done = installed}
        <p class="news">
          Updated from {done.from}.
          <button class="link" onclick={() => releaseNotes(done.version)}>What's new in {done.version}</button>
        </p>
      {/if}
      {#if update}
        {@const found = update}
        <p class="news">
          <button class="link" onclick={() => releaseNotes(found.version)}>What's new in {found.version}</button>
        </p>
      {/if}
      <label class="row">
        <span>
          <span class="label">Check for updates automatically</span>
          <span class="hint">At start and twice a day; installed only when you ask</span>
        </span>
        <input
          type="checkbox"
          class="switch"
          checked={settings.check_updates}
          onchange={(e) => save({ check_updates: e.currentTarget.checked })}
        />
      </label>
      <div class="row">
        <span>
          <span class="label">Feedback</span>
          <span class="hint">Report a problem or suggest an idea, on GitHub</span>
        </span>
        <button onclick={sendFeedback}>Send Feedback…</button>
      </div>
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
  .files .path.error {
    color: var(--error);
  }
  input.text {
    width: 220px;
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--fg);
    font: inherit;
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
  }
  .buttons {
    display: flex;
    flex: none;
    gap: 6px;
  }
  code.cmd {
    display: block;
    margin-top: 4px;
    font-size: 11px;
    word-break: break-all;
    -webkit-user-select: text;
    user-select: text;
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
  button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  progress {
    width: 100%;
    margin: 2px 0 8px;
    accent-color: var(--accent);
  }
  .news {
    margin: 0 0 6px;
    font-size: 12px;
    color: var(--muted);
  }
  button.link {
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font-size: inherit;
    cursor: pointer;
  }
  .hint.error {
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
