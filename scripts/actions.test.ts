// The actions on the items of the Feeds tab: their names, when they apply, the menus' title case.
// node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { buttonText, feedActions, titled, type Item } from "../src/lib/feeds/actions.ts";

const done: [string, unknown[]][] = [];
let focus: string | null = null;
const actions = feedActions({
  open: (ids) => done.push(["open", ids]),
  add: (ids, pdf) => done.push(["add", [ids, pdf]]),
  find: (id) => done.push(["find", [id]]),
  focus: () => focus,
  triage: (action, ids) => done.push([action, ids]),
  keepUnread: (ids) => done.push(["keepUnread", ids]),
  later: (ids) => done.push(["later", ids]),
  clearReminder: (ids) => done.push(["clearReminder", ids]),
});

const item = (id: string, triage: Item["triage"] = "new", over: Partial<Item> = {}): Item => ({ id, kind: "paper", triage, ...over });

test("a toggle says which way it goes", () => {
  assert.equal(actions.star.label([item("a"), item("b", "starred")]), "Star");
  assert.equal(actions.star.label([item("a", "starred")]), "Unstar");
  assert.equal(actions.read.label([item("a", "seen"), item("b")]), "Mark read");
  assert.equal(actions.read.label([item("a", "seen")]), "Mark unread");
  assert.equal(actions.dismiss.label([item("a", "dismissed")]), "Restore");
  assert.equal(actions.dismiss.label([item("a", "dismissed"), item("b")]), "Dismiss");
  assert.equal(actions.open.label([item("a", "new", { kind: "news" })]), "Open post");
  assert.equal(actions.open.label([item("a", "new", { kind: "news" }), item("b")]), "Open");
});

test("when an action applies", () => {
  // nothing to mark read or unread: starred items keep their triage
  assert.equal(actions.read.enabled([item("a", "starred")]), false);
  assert.equal(actions.read.enabled([item("a", "seen")]), true);
  assert.equal(actions.find.enabled([item("a", "new", { kind: "blog" })]), true);
  assert.equal(actions.find.enabled([item("a")]), false);
  assert.equal(actions.clearReminder.enabled([item("a")]), false);
  assert.equal(actions.clearReminder.enabled([item("a", "new", { reminder: "snoozed" })]), true);
  // a row does not say its link; the details do: a post's page by its link, a paper's by its link or DOI
  assert.equal(actions.open.enabled([item("a")]), true);
  assert.equal(actions.open.enabled([item("a", "new", { url: null, doi: null })]), false);
  assert.equal(actions.open.enabled([item("a", "new", { url: null, doi: "10.1/x" })]), true);
  assert.equal(actions.open.enabled([item("a", "new", { kind: "news", url: null, doi: "10.1/x" })]), false);
  assert.equal(actions.open.enabled([item("a", "new", { kind: "news", url: "https://x.org" })]), true);
});

test("what the actions run, and their keys", () => {
  done.length = 0;
  actions.star.run(["a"]);
  actions.addBare.run(["b"]);
  actions.find.run(["c", "d"]);
  // the item in focus, when it is one of them
  focus = "d";
  actions.find.run(["c", "d"]);
  focus = "z";
  actions.find.run(["c", "d"]);
  assert.deepEqual(done, [
    ["star", ["a"]],
    ["add", [["b"], false]],
    ["find", ["c"]],
    ["find", ["d"]],
    ["find", ["c"]],
  ]);
  const keys = Object.values(actions).flatMap((a) => (a.key ? [a.key] : []));
  assert.deepEqual([...keys].sort(), ["a", "f", "m", "o", "s", "u", "x", "z"]);
});

test("the menus in title case, the buttons with their marks", () => {
  assert.equal(titled("Mark all above as read"), "Mark All Above as Read");
  assert.equal(titled("Add without PDF…"), "Add Without PDF…");
  assert.equal(titled("Snooze 1 day"), "Snooze 1 Day");
  assert.equal(titled("Save for later"), "Save for Later");
  assert.equal(buttonText("star", "Unstar"), "★ Unstar");
  assert.equal(buttonText("star", "Star"), "☆ Star");
  assert.equal(buttonText("later", "Later"), "⏰ Later ▾");
  assert.equal(buttonText("open", "Open post"), "Open post ↗");
  assert.equal(buttonText("read", "Mark read"), "Mark read");
});
