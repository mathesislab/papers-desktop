<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, tick } from "svelte";
  import type { Item, Status } from "$lib/types";

  let query = $state("");
  let items = $state<Item[]>([]);
  let selected = $state(0);
  let status = $state<Status>({ state: "loading" });
  let input: HTMLInputElement;
  let list = $state<HTMLUListElement>();

  async function search() {
    const q = query;
    const found = await invoke<Item[]>("search", { query: q });
    if (q !== query) return; // a newer search is on its way
    items = found;
    selected = 0;
    list?.scrollTo(0, 0);
  }

  async function select(i: number) {
    selected = Math.max(0, Math.min(items.length - 1, i));
    await tick();
    list?.children[selected]?.scrollIntoView({ block: "nearest" });
  }

  function onkeydown(e: KeyboardEvent) {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        select(selected + 1);
        break;
      case "ArrowUp":
        e.preventDefault();
        select(selected - 1);
        break;
      case "Escape":
        e.preventDefault();
        invoke("hide_panel");
        break;
    }
  }

  onMount(() => {
    invoke<Status>("status").then((s) => (status = s));
    const unlisten = [
      listen<Status>("status", (e) => {
        status = e.payload;
        if (status.state === "ready") search();
      }),
      listen("shown", () => {
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

  {#if items.length}
    <ul bind:this={list} role="listbox">
      {#each items as item, i (item.key)}
        <!-- the keys are handled on the window -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          role="option"
          aria-selected={i === selected}
          tabindex="-1"
          class:selected={i === selected}
          onclick={() => select(i)}
        >
          <div class="text">
            <div class="title">{item.title}</div>
            <div class="subtitle">{item.subtitle}</div>
          </div>
          {#if item.files.length}
            <span class="badge">PDF</span>
          {:else if item.doi}
            <span class="badge">DOI</span>
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <div class="empty">
      {#if query.trim() && status.state === "ready"}No match{/if}
    </div>
  {/if}

  <footer class:error={status.state === "failed"}>
    {#if status.state === "loading"}
      Loading library…
    {:else if status.state === "ready"}
      {status.count} papers
    {:else}
      {status.error}
    {/if}
  </footer>
</main>

<style>
  :global(:root) {
    --bg: rgba(246, 246, 248, 0.98);
    --fg: #1d1d1f;
    --muted: #6e6e73;
    --border: rgba(0, 0, 0, 0.14);
    --line: rgba(0, 0, 0, 0.08);
    --accent: #2f6fde;
    --on-accent: #ffffff;
    --error: #c4321f;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg: rgba(38, 38, 40, 0.98);
      --fg: #f2f2f4;
      --muted: #9a9aa0;
      --border: rgba(255, 255, 255, 0.16);
      --line: rgba(255, 255, 255, 0.08);
      --accent: #3a78e6;
      --error: #ff7a66;
    }
  }
  :global(html, body) {
    margin: 0;
    height: 100%;
    overflow: hidden;
    background: transparent;
    color: var(--fg);
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
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

  ul {
    flex: 1;
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
  li.selected {
    background: var(--accent);
    color: var(--on-accent);
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
  }
  li.selected .subtitle {
    color: inherit;
    opacity: 0.85;
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
    align-items: center;
    justify-content: center;
    color: var(--muted);
    font-size: 14px;
  }

  footer {
    flex: none;
    padding: 6px 18px;
    border-top: 1px solid var(--line);
    color: var(--muted);
    font-size: 11px;
    white-space: pre-wrap;
    -webkit-user-select: text;
    user-select: text;
  }
  footer.error {
    color: var(--error);
  }
</style>
