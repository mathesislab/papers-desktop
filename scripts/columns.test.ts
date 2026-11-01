// The columns of the tables: their grid, the widths drawn, how an edge resizes (each edge of the three tables), how
// a header moves its column, the menu, the layout of an older version read. node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { FEED_ORDER, FEED_SPECS, SEARCH_ORDER, SEARCH_SPECS } from "../src/lib/feeds/columns.ts";
import { COLUMNS, DEFAULT_ORDER } from "../src/lib/library/columns.ts";
import {
  columnsOf,
  drawnOf,
  flexibleOf,
  gridOf,
  leastOf,
  moved,
  resized,
  restoredColumns,
  toggled,
  widthOf,
  widthsOf,
  type Shown,
  type Specs,
} from "../src/lib/list/columns.ts";

type K = "score" | "title" | "authors" | "date";
const specs: Specs<K> = {
  score: { name: "Score", width: 60, least: 60 },
  title: { name: "Title", width: 240, least: 100 },
  authors: { name: "Authors", width: 150 },
  date: { name: "Date", width: 58, least: 58 },
};
const order: K[] = ["score", "title", "authors", "date"];
const shown = columnsOf(order, specs);

test("the grid: the title takes the room left, a width under its least drawn at its least", () => {
  assert.equal(gridOf([22, 18], shown, specs), "22px 18px 60px minmax(240px, 1fr) 150px 58px");
  assert.equal(gridOf([], [{ key: "score", width: 10 }, { key: "date", width: 70 }], specs), "60px minmax(70px, 1fr)");
  assert.equal(widthOf([22, 18], shown, specs), 22 + 18 + 60 + 240 + 150 + 58);
});

const total = (widths: number[]) => widths.reduce((a, w) => a + w, 0);

test("the widths drawn: the flexible column takes the room left, none when the columns are wider", () => {
  assert.deepEqual(drawnOf(shown, specs, 608), [60, 340, 150, 58]);
  assert.deepEqual(drawnOf(shown, specs, 400), [60, 240, 150, 58]);
  // no title: the last column takes it
  assert.deepEqual(drawnOf(columnsOf(["score", "date"], specs), specs, 200), [60, 140]);
});

test("an edge resizes the column to its left; those after follow; the last gives the room or takes it", () => {
  // 608 wide: the title drawn 340, its least 240
  const room = 608;
  // the score's edge: the score wider, the date narrower to its least, then the table wider than the room
  assert.deepEqual(drawnOf(resized(shown, specs, room, 0, -10), specs, room), [60, 340, 150, 58]);
  assert.deepEqual(drawnOf(resized(shown, specs, room, 0, 25), specs, room), [85, 340, 150, 58]);
  // the authors' edge, after the title: the title stays as drawn
  assert.deepEqual(drawnOf(resized(shown, specs, room, 2, -40), specs, room), [60, 340, 110, 98]);
  assert.deepEqual(drawnOf(resized(shown, specs, room, 2, 30), specs, room), [60, 340, 180, 58]);
  // the title's own edge: the title narrower, the date taking the room; wider, the table scrolls across
  assert.deepEqual(drawnOf(resized(shown, specs, room, 1, -100), specs, room), [60, 240, 150, 158]);
  assert.deepEqual(drawnOf(resized(shown, specs, room, 1, -300), specs, room), [60, 100, 150, 298]);
  assert.deepEqual(drawnOf(resized(shown, specs, room, 1, 20), specs, room), [60, 360, 150, 58]);
  // the last edge: the date wider, the table across; not narrower than the room
  assert.deepEqual(drawnOf(resized(shown, specs, room, 3, 12), specs, room), [60, 340, 150, 70]);
  assert.deepEqual(drawnOf(resized(shown, specs, room, 3, -12), specs, room), [60, 340, 150, 58]);
});

test("the title's width kept: its least, as before, unless the table grows wider than the room", () => {
  const room = 608;
  // the room given back to the title: the window narrowed, it narrows to its least again
  assert.equal(resized(shown, specs, room, 2, -40)[1].width, 240);
  assert.equal(resized(shown, specs, room, 1, -50)[1].width, 240);
  // its least lowered with it
  assert.equal(resized(shown, specs, room, 1, -150)[1].width, 190);
  // wider than the room: the title as it was drawn, or as its edge left it
  assert.equal(resized(shown, specs, room, 2, 30)[1].width, 340);
  assert.equal(resized(shown, specs, room, 1, 50)[1].width, 390);
  // the window narrower than the table: the widths as they were, the last edge narrowing the last column down to
  // its least, or to the room
  assert.deepEqual(drawnOf(resized(shown, specs, 300, 3, -100), specs, 300), [60, 240, 150, 58]);
  const wide = columnsOf(order, specs).map((c) => (c.key === "date" ? { ...c, width: 120 } : c));
  assert.deepEqual(drawnOf(resized(wide, specs, 540, 3, -100), specs, 540), [60, 240, 150, 90]);
});

/** The three tables' columns: as at first, moved about, some hidden, the title hidden. */
function arrangements<C extends string>(specs: Specs<C>, order: C[]): Shown<C>[][] {
  const all = Object.keys(specs) as C[];
  const without = (keys: C[], key: C) => keys.filter((k) => k !== key);
  const title = "title" as C;
  const lists: C[][] = [
    order,
    [...order].reverse(),
    [title, ...without(order, title)],
    [...without(order, title), title],
    without(order, title),
    without(order, order[order.length - 1]),
    all,
    [order[0]],
    [title],
  ];
  // stored widths of every kind: at first, under their least, wide
  return lists.flatMap((keys) => [
    columnsOf(keys, specs),
    keys.map((key, i) => ({ key, width: i % 2 ? 10 : specs[key].width + 40 })),
  ]);
}

const TABLES: [string, Specs<string>, string[]][] = [
  ["library", COLUMNS, DEFAULT_ORDER],
  ["feeds", FEED_SPECS, FEED_ORDER],
  ["search", SEARCH_SPECS, SEARCH_ORDER],
];

test("every edge of the three tables, the columns moved about or hidden, in a room wider or narrower", () => {
  for (const [table, specs, order] of TABLES) {
    for (const columns of arrangements(specs, order)) {
      const width = total(widthsOf(columns, specs));
      for (const room of [width + 300, width + 1, width, width - 150]) {
        const drawn = drawnOf(columns, specs, room);
        const last = columns.length - 1;
        const least = (i: number) => leastOf(specs, columns[i].key);
        for (let index = 0; index <= last; index++) {
          for (const move of [-600, -45, -1, 0, 1, 45, 600]) {
            const at = `${table} ${columns.map((c) => c.key)} in ${room}, edge ${index} by ${move}`;
            const out = resized(columns, specs, room, index, move);
            const now = drawnOf(out, specs, room);
            assert.deepEqual(out.map((c) => c.key), columns.map((c) => c.key), at);
            // the columns before the edge do not move
            assert.deepEqual(now.slice(0, index), drawn.slice(0, index), at);
            // the column to its left resized, not under its least
            const sized = Math.max(least(index), drawn[index] + move);
            if (index < last) {
              assert.equal(now[index], sized, at);
              // those after it keep their widths, the last giving the room or taking it
              assert.deepEqual(now.slice(index + 1, last), drawn.slice(index + 1, last), at);
              assert.equal(now[last], Math.max(least(last), drawn[last] - (sized - drawn[index])), at);
            } else {
              // the last: the table no narrower than the room
              assert.equal(now[last], Math.max(sized, room - total(drawn.slice(0, last))), at);
            }
            // no column under its least, the table no narrower than the room
            now.forEach((w, i) => assert.ok(w >= least(i), at));
            assert.ok(total(now) >= room, at);
            // the widths kept draw the same after a reload
            const saved = restoredColumns(JSON.parse(JSON.stringify(out)), specs, order);
            assert.deepEqual(drawnOf(saved, specs, room), now, at);
            // the window wider than the table: the flexible column takes the room
            const flexible = flexibleOf(out);
            const wider = drawnOf(out, specs, total(now) + 100);
            assert.deepEqual(wider, now.map((w, i) => (i === flexible ? w + 100 : w)), at);
          }
          // a press without a move moves nothing
          assert.deepEqual(drawnOf(resized(columns, specs, room, index, 0), specs, room), drawn, `${table} ${index}`);
        }
      }
    }
  }
});

test("a header dragged moves its column", () => {
  assert.deepEqual(moved(shown, 0, 2).map((c) => c.key), ["title", "score", "authors", "date"]);
  assert.deepEqual(moved(shown, 3, 0).map((c) => c.key), ["date", "score", "title", "authors"]);
  assert.deepEqual(moved(shown, 1, 4).map((c) => c.key), ["score", "authors", "date", "title"]);
  // dropped where it is
  assert.equal(moved(shown, 1, 1), shown);
  assert.equal(moved(shown, 1, 2), shown);
});

test("the menu shows a column at the end, hides one, keeps the last", () => {
  assert.deepEqual(toggled(shown, specs, "authors").map((c) => c.key), ["score", "title", "date"]);
  const one = [{ key: "title" as K, width: 300 }];
  assert.deepEqual(toggled(one, specs, "title"), one);
  assert.deepEqual(toggled(one, specs, "date"), [{ key: "title", width: 300 }, { key: "date", width: 58 }]);
});

test("the layout of an older version: the widths by column become a list in the first order, widths kept", () => {
  assert.deepEqual(restoredColumns({ score: 80, title: 300, authors: 120, date: 58, gone: 9 }, specs, order), [
    { key: "score", width: 80 },
    { key: "title", width: 300 },
    { key: "authors", width: 120 },
    { key: "date", width: 58 },
  ]);
  // a width missing or wrong takes the one at first
  assert.deepEqual(restoredColumns({ title: "x", authors: -4 }, specs, ["title", "authors"]), [
    { key: "title", width: 240 },
    { key: "authors", width: 150 },
  ]);
  // the library's list of `{column, width}`; a column no longer known, or twice, left out
  assert.deepEqual(
    restoredColumns([{ column: "title", width: 333 }, { column: "pages", width: 9 }, { column: "title", width: 1 }, { column: "date", width: 70.4 }], specs, order),
    [
      { key: "title", width: 333 },
      { key: "date", width: 70 },
    ],
  );
  // as saved now
  assert.deepEqual(restoredColumns([{ key: "authors", width: 99 }], specs, order), [{ key: "authors", width: 99 }]);
  // nothing, or nothing left: the columns at first
  assert.deepEqual(restoredColumns(undefined, specs, order), shown);
  assert.deepEqual(restoredColumns([{ column: "pages", width: 9 }], specs, order), shown);
  assert.deepEqual(restoredColumns("x", specs, order), shown);
});
