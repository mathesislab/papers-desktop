// The row in focus scrolled into view: a step away just enough, farther a third of the way down.
// node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { scrolledTo } from "../src/lib/list/virtual.svelte.ts";

test("the row in focus in view: as it is, a step, or a third of the way down", () => {
  // 10 rows of 20 px in view from 200 (rows 10–19)
  assert.equal(scrolledTo(260, 20, 200, 200), 200, "in view");
  assert.equal(scrolledTo(400, 20, 200, 200), 220, "the next row: one step");
  assert.equal(scrolledTo(180, 20, 200, 200), 180, "the row before: one step");
  assert.equal(scrolledTo(2000, 20, 200, 200), 2000 - 66, "far below: a third down");
  assert.equal(scrolledTo(20, 20, 2000, 200), 0, "far above, near the top: the top");
});
