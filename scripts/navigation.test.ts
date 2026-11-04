// Back and forward in the main window: the places shown, a paper closed.
// node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { HISTORY_MAX, NO_HISTORY, goBack, goForward, kept, placeOf, placeText, went, type Place } from "../src/lib/navigation.ts";

const feeds: Place = { tab: "feeds", paper: null };
const list: Place = { tab: "library", paper: null };
const pdf = (id: string): Place => ({ tab: "papers", paper: id });

test("a place: the tab, and in the Papers tab the PDF in front", () => {
  assert.deepEqual(placeOf("feeds", "a"), feeds);
  assert.deepEqual(placeOf("papers", "a"), pdf("a"));
  assert.deepEqual(placeOf("library", "a"), list, "the library's list, whatever paper the Papers tab has");
});

test("back and forward, as a browser's: a new place drops the places forward", () => {
  let h = went(NO_HISTORY, feeds, pdf("a"));
  h = went(h, pdf("a"), list);
  assert.equal(went(h, list, list), h, "the same place is no step");
  const back = goBack(h, list)!;
  assert.deepEqual(back.to, pdf("a"));
  const again = goBack(back.history, pdf("a"))!;
  assert.deepEqual(again.to, feeds);
  assert.equal(goBack(again.history, feeds), null, "at the start");
  const forward = goForward(again.history, feeds)!;
  assert.deepEqual(forward.to, pdf("a"));
  assert.deepEqual(forward.history.forward, [list]);
  // elsewhere from there: no forward any more
  assert.deepEqual(went(forward.history, pdf("a"), pdf("b")).forward, []);
  assert.equal(goForward(NO_HISTORY, feeds), null, "at the end");
});

test("the history keeps its last places", () => {
  let h = NO_HISTORY;
  for (let i = 0; i < HISTORY_MAX + 10; i++) h = went(h, pdf(`p${i}`), pdf(`p${i + 1}`));
  assert.equal(h.back.length, HISTORY_MAX);
  assert.deepEqual(h.back[0], pdf("p10"));
});

test("a paper closed: its places go, the places left side by side once", () => {
  const h = { back: [feeds, pdf("a"), feeds, pdf("b")], forward: [pdf("a"), list, pdf("a")] };
  assert.deepEqual(kept(h, ["b"], list), { back: [feeds, pdf("b")], forward: [] });
  assert.deepEqual(kept(h, [], feeds), { back: [], forward: [list] });
  assert.deepEqual(kept(h, ["a", "b"], list), h);
});

test("a place in words", () => {
  const titles: Record<string, string> = { a: "Ice", long: "x".repeat(80) };
  const titleOf = (id: string) => titles[id];
  assert.equal(placeText(feeds, titleOf), "Feeds");
  assert.equal(placeText(list, titleOf), "the Library");
  assert.equal(placeText(pdf("a"), titleOf), "“Ice”");
  assert.equal(placeText(pdf("long"), titleOf).length, 62);
  assert.equal(placeText(pdf("gone"), titleOf), "a paper");
});
