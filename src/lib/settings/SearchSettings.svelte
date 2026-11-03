<script lang="ts">
  // The Search tab of the settings window: the hotkey of the panel, recorded from the keys pressed (and the one of
  // the selection on Linux; Services on macOS), how the words match, where Enter opens a PDF; the services of
  // *Search online*.
  import { hotkeyLabel, mac } from "$lib/keys";
  import "./settings.css";
  import type { Preferences, Settings } from "./settings.svelte";

  /** `shown`: whether its tab is shown; a hotkey being recorded stops when it is not */
  let { prefs, shown }: { prefs: Preferences; shown: boolean } = $props();
  let settings = $derived(prefs.settings!);

  /** the hotkey whose keys are being recorded */
  type HotkeyField = "hotkey" | "selection_hotkey";
  let recording = $state<HotkeyField | null>(null);

  $effect(() => {
    if (!shown) recording = null;
  });

  /** While recording, the keys pressed become the hotkey, before the window's own keys; Esc gives up. */
  function record(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      recording = null;
      return;
    }
    const key = keyName(e.code);
    if (!key) return; // a modifier, so far
    const modifiers = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Cmd"];
    const held = modifiers.filter((m): m is string => !!m);
    if (!held.length) {
      prefs.show(`A hotkey needs a modifier: ${mac ? "⌘, ⌥, ⌃ or ⇧" : "Ctrl, Alt or Shift"}`, true);
      return;
    }
    const field = recording;
    recording = null;
    prefs.save({ [field]: [...held, key].join("+") });
  }

  /** The name of a key for the hotkey ("KeyP" is P), or none for a modifier. */
  function keyName(code: string): string | null {
    if (/^(Meta|OS|Control|Alt|Shift)(Left|Right)?$|^(Fn|CapsLock)$/.test(code)) return null;
    return code.replace(/^Key/, "").replace(/^Digit/, "");
  }

  /** a service of *Search online* being added */
  let service = $state({ name: "", url: "" });

  /** The service typed, after the others: saved, the fields emptied; refused, said, the fields kept. */
  async function addService() {
    const typed = { name: service.name.trim(), url: service.url.trim() };
    if (await prefs.save({ search_services: [...settings.search_services, typed] })) service = { name: "", url: "" };
  }
</script>

<svelte:window onkeydowncapture={record} />

{#snippet recorder(field: HotkeyField, keys: string)}
  <button
    class="hotkey"
    class:recording={recording === field}
    onclick={() => (recording = recording === field ? null : field)}
  >
    {recording === field ? "Press the keys…" : hotkeyLabel(keys)}
  </button>
{/snippet}

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
          >In any app: Services → Search Papers. System Settings → Keyboard → Keyboard Shortcuts → Services gives it
          a shortcut.</span
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
      onchange={(e) => prefs.save({ matching: e.currentTarget.checked ? "inside-words" : "word-start" })}
    />
  </label>
  <label class="row">
    <span>
      <span class="label">Open PDF in</span>
      <span class="hint">What Enter does in the panel; {mac ? "⌥" : "Alt+"}Enter opens it the other way</span>
    </span>
    <select
      value={settings.open_pdf}
      onchange={(e) => prefs.save({ open_pdf: e.currentTarget.value as Settings["open_pdf"] })}
    >
      <option value="app">Papers</option>
      <option value="system">The default app</option>
      {#if mac}<option value="preview">Preview</option>{/if}
    </select>
  </label>
</section>

<section>
  <h2>Search online</h2>
  <p class="hint">
    The services of the Search tab's <b>Online ▾</b>: the query opened in the browser. Google Scholar is
    <b>Online</b> itself; the others built in are in the menu when ticked.
  </p>
  <div class="services">
    {#each prefs.services.filter((name) => name !== "Google Scholar") as name (name)}
      <label>
        <input
          type="checkbox"
          checked={!settings.search_services_hidden.includes(name)}
          onchange={(e) =>
            prefs.save({
              search_services_hidden: e.currentTarget.checked
                ? settings.search_services_hidden.filter((h) => h !== name)
                : [...settings.search_services_hidden, name],
            })}
        />
        {name}
      </label>
    {/each}
  </div>
  <p class="hint">
    Your own, after them. In the address, <code>{"{query}"}</code> is the query as words, <code>{"{scholar}"}</code>
    as Google Scholar takes it (<code>author:</code>, <code>intitle:</code>, <code>source:</code>).
  </p>
  <ul class="files">
    {#each settings.search_services as s, i (s.name)}
      <li>
        <span class="file" title={s.url}>
          <span>{s.name}</span>
          <span class="path">{s.url}</span>
        </span>
        <button
          class="remove"
          title="Remove the service"
          onclick={() => prefs.save({ search_services: settings.search_services.filter((_, j) => j !== i) })}
          >✕</button
        >
      </li>
    {/each}
  </ul>
  <form
    class="service"
    onsubmit={(e) => {
      e.preventDefault();
      addService();
    }}
  >
    <input class="text name" type="text" placeholder="Name" spellcheck="false" bind:value={service.name} />
    <input
      class="text"
      type="text"
      placeholder={"https://…?q={query}"}
      spellcheck="false"
      autocomplete="off"
      bind:value={service.url}
    />
    <button type="submit" disabled={!service.name.trim() || !service.url.trim()}>Add</button>
  </form>
</section>

<style>
  .hotkey {
    min-width: 120px;
    font-variant-numeric: tabular-nums;
  }
  .hotkey.recording {
    border-color: var(--accent);
    color: var(--accent);
  }

  /* the services built in, ticked when in the menu: in columns */
  .services {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 4px 12px;
    margin-bottom: 8px;
    font-size: 13px;
  }
  .services label {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  /* a service of Search online being added: its name, its address taking the room */
  .service {
    display: flex;
    gap: 6px;
    margin-bottom: 8px;
  }
  .service input.text {
    flex: 1;
    width: auto;
    min-width: 0;
  }
  .service input.name {
    flex: none;
    width: 110px;
  }
</style>
