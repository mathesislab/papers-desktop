// The columns of the tables: their grid, how an edge resizes, how a header moves its column, the menu, the layout
// of an older version read. node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { columnsOf, gridOf, moved, resized, restoredColumns, toggled, widthOf, type Specs } from "../src/lib/list/columns.ts";

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

test("an edge before the title resizes its column; the title takes the room", () => {
  const drawn = [60, 400, 150, 58];
  assert.deepEqual(resized(shown, specs, drawn, 0, 25, true).map((c) => c.width), [85, 240, 150, 58]);
  // not under its least
  assert.deepEqual(resized(shown, specs, drawn, 0, -40, true).map((c) => c.width), [60, 240, 150, 58]);
});

test("an edge after the title, which fills the room, resizes the column after it, the title giving the room", () => {
  const drawn = [60, 400, 150, 58];
  // the title's edge to the right: the authors narrower, the title wider (its least stays)
  assert.deepEqual(resized(shown, specs, drawn, 1, 30, true).map((c) => c.width), [60, 240, 120, 58]);
  // to the left: the authors wider, the title narrower, its least going down with it once under it
  assert.deepEqual(resized(shown, specs, drawn, 1, -200, true).map((c) => c.width), [60, 200, 350, 58]);
  // no further than the title's least and the column's
  assert.deepEqual(resized(shown, specs, drawn, 1, -500, true).map((c) => c.width), [60, 100, 450, 58]);
  assert.deepEqual(resized(shown, specs, drawn, 2, 500, true).map((c) => c.width), [60, 240, 150, 58]);
  // with the title at its least, the edge resizes the column before it and the table grows
  assert.deepEqual(resized(shown, specs, [60, 240, 150, 58], 2, 20, false).map((c) => c.width), [60, 240, 170, 58]);
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
