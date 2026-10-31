<script lang="ts">
  // The ★ next to the search field of a tab: the filters shown saved under a name, typed in a field that opens
  // under it (Enter saves, Esc or a click elsewhere closes). Filled when a saved search is what the tab shows.

  import { tick } from "svelte";

  let {
    shown,
    suggestion,
    refusal,
    onsave,
  }: {
    /** a saved search is what the tab shows */
    shown: boolean;
    /** the name offered: the filters in a few words */
    suggestion: string;
    /** why a name cannot be given; none: it can */
    refusal: (name: string) => string | null;
    onsave: (name: string) => void;
  } = $props();

  let naming = $state(false);
  let name = $state("");
  let field = $state<HTMLInputElement>();
  let refused = $derived(refusal(name));

  async function open() {
    name = suggestion;
    naming = true;
    await tick();
    field?.select();
  }

  function save(e: SubmitEvent) {
    e.preventDefault();
    if (refused) return;
    naming = false;
    onsave(name.trim());
  }

  function key(e: KeyboardEvent) {
    // the keys of the tab (arrows, Enter, Escape, the triage) are not for the field
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      naming = false;
    }
  }
</script>

<div class="save">
  <button
    class="star"
    class:on={shown}
    class:open={naming}
    title={shown ? "A saved search shows this; save it under another name" : "Save the search, with the filters chosen"}
    aria-label="Save the search"
    onclick={() => (naming ? (naming = false) : open())}>{shown ? "★" : "☆"}</button
  >
  {#if naming}
    <button class="backdrop" aria-label="Close" onclick={() => (naming = false)}></button>
    <form class="pop" onsubmit={save}>
      <label for="saved-name">Save the search as</label>
      <div class="line">
        <input
          id="saved-name"
          bind:this={field}
          bind:value={name}
          spellcheck="false"
          autocomplete="off"
          onkeydown={key}
        />
        <button type="submit" disabled={refused !== null}>Save</button>
      </div>
      {#if refused && name.trim()}<p class="note">{refused}</p>{/if}
    </form>
  {/if}
</div>

<style>
  .save {
    position: relative;
    flex: none;
  }
  .star {
    width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: none;
    color: var(--muted);
    font-size: 14px;
  }
  .star.on {
    color: #d9822b;
  }
  .star.open,
  .star:hover {
    background: var(--line);
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 2;
    border: none;
    background: none;
  }
  .pop {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 3;
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 260px;
    padding: 8px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--menu-bg);
    font-size: 12px;
  }
  label {
    color: var(--muted);
  }
  .line {
    display: flex;
    gap: 6px;
  }
  input {
    flex: 1;
    min-width: 0;
    height: 24px;
    padding: 0 6px;
    border: 1px solid var(--accent);
    border-radius: 5px;
    background: transparent;
    color: var(--fg);
    font: inherit;
    outline: none;
    -webkit-user-select: text;
    user-select: text;
  }
  .line button {
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: none;
    color: var(--fg);
    font: inherit;
  }
  .line button:disabled {
    color: var(--muted);
  }
  .note {
    margin: 0;
    color: var(--error);
  }
</style>
