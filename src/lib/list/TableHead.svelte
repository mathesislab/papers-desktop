<script lang="ts" generics="K extends string">
  // The headers of a table of the three tabs (the library's Table, FeedTable), after the columns without one
  // (`lead`), at the top of the table as it scrolls: a click on a header sorts by it (the table's `onclick`), a
  // header dragged moves its column, an edge dragged resizes the column to its left (`resized`: the columns after it
  // follow, the last giving the room), a right-click shows or hides columns. The window's layout keeps the columns.

  import { CheckMenuItem, Menu } from "@tauri-apps/api/menu";
  import { gridOf, moved, resized, toggled, widthOf, type Shown, type Specs } from "./columns";

  let {
    lead,
    room,
    columns,
    specs,
    menu,
    heading,
    onclick,
    oncolumns,
  }: {
    /** the widths of the columns before, without a header (a mark, the star) */
    lead: number[];
    /** the width of the table in view */
    room: number;
    columns: Shown<K>[];
    specs: Specs<K>;
    /** the columns the menu offers, in its order */
    menu: K[];
    /** a header's name (none: the column's), its arrow, what it says on hover, whether the table is sorted by it */
    heading: (key: K) => { name?: string; arrow?: string; title?: string; sorted?: boolean };
    /** a header clicked, not dragged */
    onclick: (key: K) => void;
    oncolumns: (columns: Shown<K>[]) => void;
  } = $props();

  /** a header moved this far is dragged, not clicked */
  const DRAG = 4;

  let head = $state<HTMLDivElement>();

  let template = $derived(gridOf(lead, columns, specs));
  let minWidth = $derived(widthOf(lead, columns, specs));

  /** a header pressed: dragged to move its column, or clicked */
  let press: { index: number; x: number; dragging: boolean } | null = null;
  /** where the column dragged would go: before the column of this index */
  let dropAt = $state<number | null>(null);

  function headerDown(e: PointerEvent, index: number) {
    if (e.button !== 0) return;
    press = { index, x: e.clientX, dragging: false };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function headerMove(e: PointerEvent) {
    if (!press || !head) return;
    if (!press.dragging && Math.abs(e.clientX - press.x) < DRAG) return;
    press.dragging = true;
    const cells = [...head.querySelectorAll<HTMLElement>("[data-column]")];
    const at = cells.findIndex((cell) => {
      const box = cell.getBoundingClientRect();
      return e.clientX < box.left + box.width / 2;
    });
    dropAt = at < 0 ? cells.length : at;
  }

  function headerUp(key: K) {
    if (!press) return;
    if (!press.dragging) onclick(key);
    else if (dropAt !== null) oncolumns(moved(columns, press.index, dropAt));
    press = null;
    dropAt = null;
  }

  /** The edge after a column pressed: dragged, it resizes the columns as they were then, in the room then. The
   *  window follows the pointer, the edge keeping it (its cursor) while it is there. */
  function edgeDown(e: PointerEvent, index: number) {
    if (e.button !== 0) return;
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const [x, start, space] = [e.clientX, columns, room - lead.reduce((sum, w) => sum + w, 0)];
    const move = (m: PointerEvent) => oncolumns(resized(start, specs, space, index, Math.round(m.clientX - x)));
    const end = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  }

  /** Right-click on the headers: the columns to show. */
  async function columnMenu(e: MouseEvent) {
    e.preventDefault();
    const items = await Promise.all(
      menu.map((key) => {
        const on = columns.some((c) => c.key === key);
        return CheckMenuItem.new({
          text: specs[key].name,
          checked: on,
          // the last column stays
          enabled: !on || columns.length > 1,
          action: () => oncolumns(toggled(columns, specs, key)),
        });
      }),
    );
    const popup = await Menu.new({ items });
    await popup.popup();
  }
</script>

<div
  class="head"
  role="row"
  tabindex="-1"
  bind:this={head}
  style:grid-template-columns={template}
  style:min-width="{minWidth}px"
  oncontextmenu={columnMenu}
>
  {#each lead as _, i (i)}<span></span>{/each}
  {#each columns as { key }, i (key)}
    {@const h = heading(key)}
    <div
      class="cell"
      class:sorted={h.sorted}
      class:drop-before={dropAt === i}
      class:drop-after={dropAt === columns.length && i === columns.length - 1}
      role="columnheader"
      tabindex="-1"
      title={h.title}
      data-column={key}
      onpointerdown={(e) => headerDown(e, i)}
      onpointermove={headerMove}
      onpointerup={() => headerUp(key)}
    >
      <span class="name">{h.name ?? specs[key].name}</span>{#if h.arrow}<span class="arrow">{h.arrow}</span>{/if}
      <span
        class="edge"
        class:last={i === columns.length - 1}
        role="separator"
        aria-orientation="vertical"
        onpointerdown={(e) => edgeDown(e, i)}
      ></span>
    </div>
  {/each}
</div>

<style>
  /* as high as a row (ROW), over the rows as they scroll */
  .head {
    position: sticky;
    top: 0;
    z-index: 2;
    display: grid;
    align-items: center;
    box-sizing: border-box;
    height: 26px;
    border-bottom: 1px solid var(--line);
    background: var(--bg);
  }
  .cell {
    position: relative;
    display: flex;
    align-items: center;
    height: 100%;
    min-width: 0;
    padding: 0 8px;
    color: var(--muted);
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
  }
  .cell.sorted {
    color: var(--fg);
  }
  /* a header keeps to its column: its name cut short first, then its arrow */
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .arrow {
    flex: none;
    margin-left: 4px;
    font-size: 10px;
  }
  .cell.drop-before {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .cell.drop-after {
    box-shadow: inset -2px 0 0 var(--accent);
  }
  .edge {
    position: absolute;
    top: 4px;
    right: -3px;
    bottom: 4px;
    z-index: 1;
    width: 6px;
    cursor: col-resize;
  }
  /* the end of the table: inside it, not wider than the room, with no line */
  .edge.last {
    right: 0;
  }
  .edge.last::after {
    content: none;
  }
  .edge::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 2px;
    width: 1px;
    background: var(--line);
  }
</style>
