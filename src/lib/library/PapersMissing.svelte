<script lang="ts">
  // A papers library is written by papers: without it (or with papers older
  // than 3.0), how to install it.

  let {
    version = null,
    onclose,
  }: {
    /** of the papers found, too old; none when papers is not found */
    version?: string | null;
    onclose: () => void;
  } = $props();

  let commands = $derived(
    version
      ? ["pip install --pre --upgrade papers-cli", "uv tool install --prerelease allow --upgrade papers-cli"]
      : ["pip install --pre papers-cli", "uv tool install --prerelease allow papers-cli"],
  );
  let copied = $state<string | null>(null);
  let close: HTMLButtonElement;

  $effect(() => close.focus());

  async function copy(command: string) {
    await navigator.clipboard.writeText(command);
    copied = command;
    setTimeout(() => (copied = null), 1200);
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<div class="backdrop">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="papers-missing-title">
    <h2 id="papers-missing-title">This library is written by papers</h2>
    <p>
      The app writes a papers library with the papers command, so that a change made here or in a terminal has
      the same format and history. {#if version}papers {version} is installed: 3.0 or later is needed.{:else}papers is not found.{/if}
    </p>
    <p>{version ? "Upgrade it" : "Install it"} with one of:</p>
    <ul>
      {#each commands as command (command)}
        <li>
          <code>{command}</code>
          <button onclick={() => copy(command)}>{copied === command ? "Copied" : "Copy"}</button>
        </li>
      {/each}
    </ul>
    <p class="note">The app finds it at the next write. Or give its path in Settings.</p>
    <div class="actions">
      <button class="primary" bind:this={close} onclick={onclose}>Close</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.25);
  }
  .dialog {
    width: min(480px, calc(100vw - 32px));
    padding: 18px 20px 16px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--menu-bg);
    color: var(--fg);
    font-size: 13px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.22);
  }
  h2 {
    margin: 0 0 10px;
    font-size: 14px;
    font-weight: 600;
  }
  p {
    margin: 0 0 10px;
    line-height: 1.45;
  }
  ul {
    margin: 0 0 10px;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }
  code {
    flex: 1;
    min-width: 0;
    padding: 5px 8px;
    border-radius: 5px;
    background: var(--line);
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
    overflow-wrap: anywhere;
    user-select: all;
  }
  .note {
    color: var(--muted);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
  button {
    flex: none;
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--on-accent);
  }
</style>
