<script lang="ts">
  // The General tab of the settings window: the start at login, the text size (also ⌘+ and ⌘- in another window,
  // `text-size`), what shows after a paper is closed, the recent papers, the citation style, what is sent to doi.org
  // and OpenAlex; the updates (`#updates`, where the app's "updates" section scrolls) and the feedback.
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { mac } from "$lib/keys";
  import "./settings.css";
  import { fileName, type Preferences, type Settings } from "./settings.svelte";
  import type { Updates } from "./updates.svelte";

  let { prefs, updates }: { prefs: Preferences; updates: Updates } = $props();
  let settings = $derived(prefs.settings!);

  /** the option of the style menu that asks for a .csl file */
  const OTHER = "\u0000other";

  // a style that is not bundled: a .csl file
  let custom = $derived(!prefs.styles.includes(settings.citation_style) ? settings.citation_style : null);

  let recentCleared = $state(false);

  onMount(() => {
    // ⌘+ and ⌘- in another window
    const unlisten = listen<Settings["text_size"]>("text-size", (e) => (settings.text_size = e.payload));
    return () => unlisten.then((f) => f());
  });

  async function toggleStartAtLogin() {
    try {
      prefs.startAtLogin = await invoke<boolean>("set_start_at_login", { on: !prefs.startAtLogin });
    } catch (err) {
      prefs.show(String(err), true);
    }
  }

  async function clearRecent() {
    await invoke("clear_recent");
    recentCleared = true;
  }

  async function chooseStyle(e: Event & { currentTarget: HTMLSelectElement }) {
    const value = e.currentTarget.value;
    if (value !== OTHER) return prefs.save({ citation_style: value });
    // the menu shows the current style until a file is chosen
    e.currentTarget.value = settings.citation_style;
    const file = await open({ title: "A CSL style", filters: [{ name: "CSL style", extensions: ["csl"] }] });
    if (typeof file === "string") await prefs.save({ citation_style: file });
  }

  async function releaseNotes(of: string) {
    try {
      await invoke("release_notes", { version: of });
    } catch (err) {
      prefs.show(String(err), true);
    }
  }

  async function sendFeedback() {
    try {
      await invoke("send_feedback");
    } catch (err) {
      prefs.show(String(err), true);
    }
  }

  const styleName = (name: string) =>
    name.split("-").map((word) => word[0].toUpperCase() + word.slice(1)).join(" ");
</script>

<section>
  <h2>General</h2>
  <label class="row">
    <span class="label">Start at login</span>
    <input type="checkbox" class="switch" checked={prefs.startAtLogin} onchange={toggleStartAtLogin} />
  </label>
  <label class="row">
    <span>
      <span class="label">Text size</span>
      <span class="hint">Zooms every window ({mac ? "⌘+ ⌘- ⌘0" : "Ctrl+ + - 0"})</span>
    </span>
    <select
      value={settings.text_size}
      onchange={(e) => prefs.save({ text_size: e.currentTarget.value as Settings["text_size"] })}
    >
      <option value="smallest">Smallest</option>
      <option value="smaller">Smaller</option>
      <option value="default">Default</option>
      <option value="larger">Larger</option>
      <option value="largest">Largest</option>
    </select>
  </label>
  <label class="row">
    <span>
      <span class="label">After closing a paper</span>
      <span class="hint">In the main window ({mac ? "⌘W" : "Ctrl+W"})</span>
    </span>
    <select
      value={settings.after_close}
      onchange={(e) => prefs.save({ after_close: e.currentTarget.value as Settings["after_close"] })}
    >
      <option value="next">Show what was shown before it</option>
      <option value="library">Show the library</option>
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
      <span class="label">Citation style</span>
      <span class="hint">Of the citation in the preview</span>
    </span>
    <select value={settings.citation_style} onchange={chooseStyle}>
      {#each prefs.styles as name (name)}<option value={name}>{styleName(name)}</option>{/each}
      {#if custom}<option value={custom}>{fileName(custom)}</option>{/if}
      <option value={OTHER}>Other .csl File…</option>
    </select>
  </label>
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
      onchange={(e) => prefs.save({ email: e.currentTarget.value.trim() })}
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
      onchange={(e) => prefs.save({ openalex_key: e.currentTarget.value.trim() })}
    />
  </label>
</section>

<section id="updates">
  <h2>Updates</h2>
  <div class="row">
    <span>
      <span class="label">Version {updates.version}</span>
      <span class="hint" class:error={updates.updating === "failed"}>{updates.status}</span>
    </span>
    {#if updates.update}
      <button class="primary" disabled={updates.updating === "installing"} onclick={updates.install}>
        Install {updates.update.version} and Restart
      </button>
    {:else}
      <button disabled={updates.updating === "checking"} onclick={updates.check}>Check for Updates Now</button>
    {/if}
  </div>
  {#if updates.updating === "installing"}
    <progress value={updates.progress.done} max={updates.progress.total ?? undefined}></progress>
  {/if}
  {#if updates.installed}
    {@const done = updates.installed}
    <p class="news">
      Updated from {done.from}.
      <button class="link" onclick={() => releaseNotes(done.version)}>What's new in {done.version}</button>
    </p>
  {/if}
  {#if updates.update}
    {@const found = updates.update}
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
      onchange={(e) => prefs.save({ check_updates: e.currentTarget.checked })}
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

<style>
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
</style>
