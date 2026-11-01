// The rows of a list as a selection moves through them: a range, a step of the keys, the row after some go.
// node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { nextAfter, rangeOf, stepped } from "../src/lib/list/order.ts";

const ids = ["a", "b", "c", "d", "e"];

test("a range, in the order of the list", () => {
  assert.deepEqual(rangeOf(ids, "c", "a"), ["a", "b", "c"]);
  assert.deepEqual(rangeOf(ids, "b", "d"), ["b", "c", "d"]);
  assert.deepEqual(rangeOf(ids, "b", "b"), ["b"]);
  // from a row no longer in the list: the row alone
  assert.deepEqual(rangeOf(ids, "z", "b"), ["b"]);
});

test("a step of the keys: some rows on, the ends kept to", () => {
  assert.equal(stepped(ids, "b", 1), "c");
  assert.equal(stepped(ids, "b", -1), "a");
  assert.equal(stepped(ids, "b", -5), "a");
  assert.equal(stepped(ids, "d", 10), "e");
  assert.equal(stepped(ids, "c", "first"), "a");
  assert.equal(stepped(ids, "c", "last"), "e");
  // from nothing in focus, or a row not in the list: the first
  assert.equal(stepped(ids, null, 1), "a");
  assert.equal(stepped(ids, "z", -1), "a");
  assert.equal(stepped([], "a", 1), null);
});

test("the row after some go: the first after the last of them that stays, else the one before", () => {
  assert.equal(nextAfter(ids, new Set(["b"])), "c");
  assert.equal(nextAfter(ids, new Set(["b", "c"])), "d");
  // the last removed counts: the next one that stays after it
  assert.equal(nextAfter(ids, new Set(["b", "d"])), "e");
  // at the end: the one before
  assert.equal(nextAfter(ids, new Set(["e"])), "d");
  assert.equal(nextAfter(ids, new Set(["d", "e"])), "c");
  assert.equal(nextAfter(ids, new Set(["c", "e"])), "d");
  assert.equal(nextAfter(ids, new Set(ids)), null);
  assert.equal(nextAfter(ids, new Set(["z"])), null);
});
