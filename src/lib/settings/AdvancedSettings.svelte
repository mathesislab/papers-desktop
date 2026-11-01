<script lang="ts">
  // The Advanced tab of the settings window: what writes the papers libraries (papers-rs built in, or a papers
  // command, and its path); the papers-rs command line tool, its link and whether ~/.local/bin is in the PATH of the
  // user's shell.
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import "./settings.css";
  import type { Preferences, Settings } from "./settings.svelte";

  let { prefs }: { prefs: Preferences } = $props();
  let settings = $derived(prefs.settings!);

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

  /** the papers-rs command line tool: its link, and whether ~/.local/bin is in the PATH of the user's shell */
  let cli = $state<Cli | null>(null);
  let cliBusy = $state(false);
  let cliPath = $state<CliPath | null>(null);

  onMount(async () => {
    cli = await invoke<Cli>("cli_status");
    // starts the shell of the user: not waited for
    findCliPath();
  });

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
      prefs.show(`Added to ${prefs.tilde(cliPath.file ?? "")}`, false);
    } catch (err) {
      prefs.show((err as CliFailure).message ?? String(err), true);
    } finally {
      cliBusy = false;
    }
  }

  let cliStatus = $derived.by(() => {
    if (!cli) return "";
    if (!cli.available) return "Not in this build of the app";
    const link = cli.link ? prefs.tilde(cli.link) : "";
    if (cli.state === "foreign") return `${link} is another program: left as it is`;
    if (cli.state === "missing") return "Not installed";
    return cli.current ? `Installed: ${link}` : `${link} points to another copy: ${prefs.tilde(cli.points_to ?? "")}`;
  });

  async function changeCli(command: "install_cli" | "uninstall_cli") {
    cliBusy = true;
    try {
      cli = await invoke<Cli>(command);
      prefs.show(command === "install_cli" ? "Installed" : "Removed", false);
      findCliPath();
    } catch (err) {
      prefs.show((err as CliFailure).message ?? String(err), true);
    } finally {
      cliBusy = false;
    }
  }
</script>

<section>
  <h2>Advanced</h2>
  <label class="row">
    <span>
      <span class="label">Writes papers libraries</span>
      <span class="hint">Built in, or a command of the papers standard; the files and the history are the same</span>
    </span>
    <select
      value={settings.writer}
      onchange={(e) => prefs.save({ writer: e.currentTarget.value as Settings["writer"] })}
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
        onchange={(e) => prefs.save({ papers: e.currentTarget.value.trim() })}
      />
    </label>
  {/if}
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
          <span class="hint">It is in {prefs.tilde(cliPath.file ?? "")} already: open a new terminal.</span>
        {/if}
      {/if}
    </span>
    {#if cli?.available && cli.state !== "foreign"}
      <span class="buttons">
        {#if cliPath && !cliPath.on_path && cliPath.file && !cliPath.added}
          <button disabled={cliBusy} onclick={addCliPath}>Add to {prefs.tilde(cliPath.file)}</button>
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
</section>

<style>
  code.cmd {
    display: block;
    margin-top: 4px;
    font-size: 11px;
    word-break: break-all;
    -webkit-user-select: text;
    user-select: text;
  }
</style>
