<script lang="ts">
  // The score of a row as two thin bars, one over the other, each filling its track from 0 to 100: its keywords on
  // top, its library under (crates/feeds' `Rating`); a part missing an empty track, both empty and tinted when an
  // exclude vetoes it. On hover, its numbers (`scoreHint`). The table's Score cell; small, at the end of a simple row
  // before its badges.

  import type { Rated, Scored } from "./api";
  import { barOf, scoreHint } from "./items";

  let { row, small = false }: { row: Rated & { interests: Scored[] }; small?: boolean } = $props();
</script>

<span class="bars" class:small class:vetoed={row.excluded !== null} title={scoreHint(row)}>
  <span class="track"><span class="bar keywords" style:width="{barOf(row.keywords)}%"></span></span>
  <span class="track"><span class="bar library" style:width="{barOf(row.library)}%"></span></span>
</span>

<style>
  .bars {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 3px;
    width: 100%;
    height: 100%;
  }
  .bars.small {
    flex: none;
    gap: 2px;
    width: 28px;
  }
  .track {
    height: 3px;
    border-radius: 2px;
    background: var(--line);
    overflow: hidden;
  }
  .small .track {
    height: 2px;
  }
  .bar {
    display: block;
    height: 100%;
  }
  .keywords {
    background: var(--accent);
  }
  /* the library's green, as the badges of a paper in it */
  .library {
    background: #2f9e6e;
  }
  .vetoed .track {
    background: color-mix(in srgb, var(--error) 30%, transparent);
  }
  :global(.row.selected) .bar {
    background: currentColor;
  }
  :global(.row.selected) .track {
    background: color-mix(in srgb, currentColor 30%, transparent);
  }
</style>
