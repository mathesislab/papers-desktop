// The order the views of the library tab were seen in: node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { closed, initial, seen, type Seen } from "../src/lib/library/history.ts";

/** The views seen one after the other, from the list alone. */
const after = (...views: Seen[]) => views.reduce(seen, [null]);

test("the view seen goes to the front", () => {
  assert.deepEqual(after("a", "b", "c"), ["c", "b", "a", null]);
  // the list is a view as a PDF is
  assert.deepEqual(after("a", "b", "c", null), [null, "c", "b", "a"]);
  // seen again: moved, not added twice
  assert.deepEqual(after("a", "b", "c", "a"), ["a", "c", "b", null]);
  assert.deepEqual(after("a", "a"), ["a", null]);
});

test("closing the PDF in front shows the view seen before it", () => {
  // A, B, C open, the list, D for a moment: closing D shows the list, not C
  const order = closed(after("a", "b", "c", null, "d"), ["d"]);
  assert.equal(order[0], null);
  // from the list to C, closing C: the list again
  assert.equal(closed(seen(order, "c"), ["c"])[0], null);
  // A, then B, closing B: A
  assert.equal(closed(after("a", "b"), ["b"])[0], "a");
});

test("closing them all goes back through the history to the list", () => {
  let order = after("a", null, "b", "c", "a");
  const shown: Seen[] = [];
  while (order[0] !== null) {
    order = closed(order, [order[0]]);
    shown.push(order[0]);
  }
  assert.deepEqual(shown, ["c", "b", null]);
  assert.deepEqual(order, [null]);
});

test("a PDF closed behind the one in front leaves it in front", () => {
  assert.deepEqual(closed(after("a", "b", "c"), ["a"]), ["c", "b", null]);
  // several at once (papers gone from the library); an id not in the order is nothing
  assert.deepEqual(closed(after("a", "b", "c"), ["c", "b", "x"]), ["a", null]);
});

test("the list is always in the order", () => {
  assert.deepEqual(seen([], "a"), ["a", null]);
  assert.deepEqual(closed(["a"], ["a"]), [null]);
});

test("the order as the window opens", () => {
  // the paper in front, the others from the last tab back, then the list
  assert.deepEqual(initial(["a", "b", "c"], "b"), ["b", "c", "a", null]);
  assert.deepEqual(initial(["a", "b"], null), [null, "b", "a"]);
  assert.deepEqual(initial([], null), [null]);
});
