<script lang="ts">
  import { tick } from "svelte";
  import { modClick } from "$lib/keys";
  import { foldTag, lastSegment, parentPart, sameTag } from "$lib/tags";
  import type { Row, TagCount } from "$lib/types";

  let {
    rows,
    tags,
    filtered,
    ontoggle,
    onset,
  }: {
    /** the papers the tags change: one, or the several selected */
    rows: Row[];
    /** the tags of the library and their parents, with the papers under each */
    tags: TagCount[];
    /** whether the search filters a tag */
    filtered: (tag: string) => boolean;
    /** a click on the name of a tag; with ⌘, `alternative` */
    ontoggle: (tag: string, alternative: boolean) => void;
    /** add and remove tags on the papers; whether it was written */
    onset: (add: string[], remove: string[]) => Promise<boolean>;
  } = $props();

  const listbox = $props.id();

  /** The tags of the papers, each once with the number of papers that have
   *  it: by count for a selection, in the order of the paper for one. */
  let chips = $derived.by(() => {
    const found = new Map<string, Pick<TagCount, "tag" | "count">>();
    for (const row of rows) {
      const own = new Map(row.tags.map((tag) => [foldTag(tag), tag]));
      for (const [folded, tag] of own) {
        const chip = found.get(folded);
        if (chip) chip.count++;
        else found.set(folded, { tag, count: 1 });
      }
    }
    const list = [...found.values()];
    return rows.length > 1 ? list.sort((a, b) => b.count - a.count || a.tag.localeCompare(b.tag)) : list;
  });

  let adding = $state(false);
  let text = $state("");
  /** the option highlighted in the list; -1: none, Enter adds the text */
  let at = $state(-1);
  let field = $state<HTMLInputElement>();
  let plus = $state<HTMLButtonElement>();
  let menu = $state<HTMLDivElement>();

  /** The tags of the library to add, parents included: not those every paper has; the ones that
   *  start with the text first, then those whose last segment does ("amoc" finds "climate/amoc"),
   *  then the ones that contain it. */
  let options = $derived.by(() => {
    const have = new Set(chips.filter((c) => c.count === rows.length).map((c) => foldTag(c.tag)));
    const word = foldTag(text.trim());
    const starts: TagCount[] = [];
    const segments: TagCount[] = [];
    const contains: TagCount[] = [];
    for (const option of tags) {
      const folded = foldTag(option.tag);
      if (have.has(folded)) continue;
      if (folded.startsWith(word)) starts.push(option);
      else if (foldTag(lastSegment(option.tag)).startsWith(word)) segments.push(option);
      else if (folded.includes(word)) contains.push(option);
    }
    return [...starts, ...segments, ...contains];
  });

  // other papers: the field closes
  let key = $derived(rows.map((r) => r.id).join("\n"));
  $effect(() => {
    key;
    close();
  });

  // typing starts again from the text
  $effect(() => {
    text;
    at = -1;
  });

  // the highlighted option stays in view
  $effect(() => {
    if (at >= 0) menu?.children[at]?.scrollIntoView({ block: "nearest" });
  });

  async function open() {
    adding = true;
    await tick();
    field?.focus();
  }

  function close() {
    adding = false;
    text = "";
  }

  /** Add a tag, in the spelling of the library when it has it. */
  async function add(tag: string) {
    const name = tag.trim();
    if (!name) return;
    const known = tags.find((t) => sameTag(t.tag, name));
    const written = await onset([known?.tag ?? name], []);
    // an error is said by the page: the text stays, to try again
    if (!written && adding && !text) text = name;
  }

  function enter() {
    const tag = options[at]?.tag ?? text;
    text = "";
    add(tag);
  }

  /** The keys of the field are its own, not those of the library (Escape,
   *  Enter and the arrows clear, open and move in the page); commands with a
   *  modifier go on. */
  function onkeydown(e: KeyboardEvent) {
    const command = (e.metaKey || e.ctrlKey) && (e.key.length === 1 || e.key === "Tab");
    if (!command) e.stopPropagation();
    if (e.isComposing) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      at = Math.min(at + 1, options.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      at = at < 0 ? options.length - 1 : at - 1;
    } else if (e.key === "Enter") {
      e.preventDefault();
      enter();
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
      tick().then(() => plus?.focus());
    }
  }
</script>

<!-- a tag with its parents dimmed -->
{#snippet path(tag: string)}<span class="parent">{parentPart(tag)}</span>{lastSegment(tag)}{/snippet}

<div class="tags">
  <div class="chips">
    {#each chips as { tag, count } (foldTag(tag))}
      <span class="chip" class:some={count < rows.length} class:on={filtered(tag)}>
        <button class="name" title="Filter by {tag}" onclick={(e) => ontoggle(tag, modClick(e))}>
          {@render path(tag)}{#if count < rows.length}<span class="count">{count}/{rows.length}</span>{/if}
        </button>
        <button
          class="remove"
          title={rows.length > 1 ? `Remove ${tag} from ${count === 1 ? "that paper" : "those papers"}` : `Remove ${tag}`}
          aria-label="Remove {tag}"
          onclick={() => onset([], [tag])}>×</button
        >
      </span>
    {/each}
    {#if adding}
      <input
        bind:this={field}
        bind:value={text}
        class="field"
        placeholder="Add a tag"
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
        role="combobox"
        aria-expanded={options.length > 0}
        aria-controls={listbox}
        aria-activedescendant={at >= 0 ? `${listbox}-${at}` : undefined}
        aria-autocomplete="list"
        {onkeydown}
        onblur={close}
      />
    {:else}
      <button bind:this={plus} class="plus" data-add-tag title="Add a tag" aria-label="Add a tag" onclick={open}>+</button>
    {/if}
  </div>
  {#if adding && (options.length || text.trim())}
    <!-- the mouse keeps the focus in the field, the scroll bar too -->
    <div
      bind:this={menu}
      class="menu"
      id={listbox}
      role="listbox"
      tabindex="-1"
      onmousedown={(e) => e.preventDefault()}
    >
      {#each options as { tag, under }, i (tag)}
        <div
          id="{listbox}-{i}"
          class="option"
          class:at={i === at}
          role="option"
          tabindex="-1"
          aria-selected={i === at}
          onmousemove={() => (at = i)}
          onclick={() => add(tag)}
          onkeydown={() => {}}
        >
          <span class="label">{@render path(tag)}</span><span class="n">{under}</span>
        </div>
      {:else}
        <div class="none">New tag: Enter adds “{text.trim()}”</div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .tags {
    position: relative;
    padding: 10px 16px 0;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    box-sizing: border-box;
    height: 22px;
    max-width: 100%;
    border: 1px solid var(--border);
    border-radius: 11px;
    font-size: 12px;
  }
  /* tags only some of the papers have */
  .chip.some {
    opacity: 0.55;
  }
  .chip.some:hover,
  .chip.some:focus-within {
    opacity: 1;
  }
  .chip.on {
    border-color: var(--accent);
    color: var(--accent);
  }
  .chip button,
  .plus {
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    cursor: default;
  }
  .name {
    min-width: 0;
    padding: 0 2px 0 9px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .parent {
    color: var(--muted);
  }
  .chip.on .parent,
  .option.at .parent {
    color: inherit;
    opacity: 0.65;
  }
  .name:hover {
    text-decoration: underline;
  }
  .name .count {
    margin-left: 5px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .chip .remove {
    flex: none;
    width: 20px;
    height: 100%;
    padding: 0;
    color: var(--muted);
    line-height: 1;
  }
  .chip .remove:hover {
    color: var(--error);
  }
  .plus {
    box-sizing: border-box;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 1px dashed var(--border);
    border-radius: 11px;
    color: var(--muted);
    font-size: 14px;
    line-height: 1;
  }
  .plus:hover {
    border-style: solid;
    color: var(--fg);
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  /* as tall as a chip: the row stays as it is when the field opens */
  /* the rest of the row: it wraps only when less than its minimum is left */
  .field {
    box-sizing: border-box;
    flex: 1 1 90px;
    min-width: 90px;
    height: 22px;
    padding: 0 9px;
    border: 1px solid var(--accent);
    border-radius: 11px;
    background: transparent;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
    outline: none;
    -webkit-user-select: text;
    user-select: text;
  }
  .menu {
    position: absolute;
    top: 100%;
    left: 16px;
    right: 16px;
    z-index: 3;
    box-sizing: border-box;
    max-height: 220px;
    margin-top: 4px;
    padding: 4px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--menu-bg);
    font-size: 12px;
    outline: none;
  }
  .option {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border-radius: 5px;
  }
  .option.at {
    background: var(--selection);
    color: var(--on-selection);
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .n {
    color: var(--muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .option.at .n {
    color: inherit;
    opacity: 0.8;
  }
  .none {
    padding: 4px 8px;
    color: var(--muted);
  }
</style>
