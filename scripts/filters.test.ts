// The combination of the filters of the library tab: node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ADDED_SHOWN,
  NO_FILTERS,
  NO_LISTS,
  chips,
  chooseLibrary,
  currentSet,
  hasTag,
  renameTags,
  restored,
  searchOf,
  tabFilters,
  tagCounts,
  toggleTag,
  viewCounts,
  without,
  type Filters,
  type Lists,
} from "../src/lib/library/filters.ts";
import { alternatives, anyOf, renameTagFilter, tagTerm } from "../src/lib/query.ts";
import type { Row } from "../src/lib/types.ts";

const row = (id: string, bib: string, tags: string[] = [], file: Row["file"] = "found"): Row =>
  ({ id, key: id, kind: "article", authors: "A", year: 2000, title: id, container: null, tags, doi: null, file, bib }) as Row;

const rows = [
  row("a1", "/x/a.bib", ["climate/amoc", "ice"], "none"),
  row("a2", "/x/a.bib", ["AMOC"], "missing"),
  row("b1", "/x/b.bib", ["climate"], "none"),
  row("b2", "/x/b.bib", [], "found"),
];
const all = new Set(rows.map((r) => r.id));
const ids = (f: Filters, matched = all, lists: Partial<Lists> = {}) =>
  currentSet(rows, matched, f, { ...NO_LISTS, ...lists }).map((r) => r.id);

test("no filters: everything the search found; nothing before the first answer", () => {
  assert.deepEqual(ids(NO_FILTERS), ["a1", "a2", "b1", "b2"]);
  assert.deepEqual(currentSet(rows, null, NO_FILTERS, NO_LISTS), []);
});

test("library, view and search combine", () => {
  assert.deepEqual(ids({ ...NO_FILTERS, bibs: ["/x/b.bib"] }), ["b1", "b2"]);
  assert.deepEqual(ids({ ...NO_FILTERS, bibs: ["/x/b.bib", "/x/a.bib"] }), ["a1", "a2", "b1", "b2"]);
  assert.deepEqual(ids({ ...NO_FILTERS, view: "nopdf" }), ["a1", "b1"]);
  assert.deepEqual(ids({ ...NO_FILTERS, view: "nopdf", bibs: ["/x/a.bib"] }), ["a1"]);
  // the core found a1 and b2 for the search
  assert.deepEqual(ids({ ...NO_FILTERS, view: "nopdf", query: "x" }, new Set(["a1", "b2"])), ["a1"]);
  assert.deepEqual(ids({ ...NO_FILTERS, view: "broken" }, new Set(["a1", "b1"])), []);
});

test("recent keeps the order of opening, duplicates their groups; both within the library", () => {
  const f: Filters = { ...NO_FILTERS, view: "recent" };
  assert.deepEqual(ids(f, all, { recent: ["b2", "a1", "gone", "b1"] }), ["b2", "a1", "b1"]);
  assert.deepEqual(ids({ ...f, bibs: ["/x/b.bib"] }, all, { recent: ["b2", "a1", "b1"] }), ["b2", "b1"]);
  const dups: Filters = { ...NO_FILTERS, view: "duplicates" };
  const duplicates = [["b1", "a2"], ["a1", "b2"]];
  assert.deepEqual(ids(dups, all, { duplicates }), ["b1", "a2", "a1", "b2"]);
  assert.deepEqual(ids({ ...dups, bibs: ["/x/a.bib"] }, all, { duplicates }), ["a2", "a1"]);
});

test("recently added: newest first, the last ones of the library, then the search", () => {
  const f: Filters = { ...NO_FILTERS, view: "added" };
  assert.deepEqual(ids(f, all, { added: ["b2", "gone", "a1"] }), ["b2", "a1"]);
  assert.deepEqual(ids({ ...f, bibs: ["/x/a.bib"] }, all, { added: ["b2", "a1"] }), ["a1"]);
  assert.deepEqual(ids(f, new Set(["a1"]), { added: ["b2", "a1"] }), ["a1"]);
  // only the last ADDED_SHOWN of the library, before the search narrows them
  const many = Array.from({ length: ADDED_SHOWN + 5 }, (_, i) => row(`n${i}`, "/x/n.bib"));
  const added = many.map((r) => r.id);
  const shown = currentSet([...rows, ...many], new Set(added), f, { ...NO_LISTS, added });
  assert.equal(shown.length, ADDED_SHOWN);
  assert.equal(shown[0].id, "n0");
  const last = new Set([added.at(-1)!]);
  assert.deepEqual(currentSet([...rows, ...many], last, f, { ...NO_LISTS, added }), []);
  assert.equal(viewCounts([...rows, ...many], { ...NO_LISTS, added }, []).added, ADDED_SHOWN);
  // the last of the libraries chosen together
  assert.equal(viewCounts([...rows, ...many], { ...NO_LISTS, added }, ["/x/a.bib", "/x/n.bib"]).added, ADDED_SHOWN);
});

test("the search sent to the core: the text, then a term per group of tags", () => {
  assert.equal(searchOf(NO_FILTERS), "");
  assert.equal(searchOf({ ...NO_FILTERS, query: "  ice  tag:x ", tags: [["amoc"], ["mass balance"]] }), 'ice  tag:x tag:amoc tag:"mass balance"');
  assert.equal(searchOf({ ...NO_FILTERS, tags: [["amoc", "ice"], ["climate/amoc"]] }), "tag:amoc,ice tag:climate/amoc");
});

test("libraries: a click chooses one, ⌘-click adds or takes out", () => {
  assert.deepEqual(chooseLibrary(["/a"], "/b", false), ["/b"]);
  assert.deepEqual(chooseLibrary(["/a"], "/a", false), ["/a"]);
  assert.deepEqual(chooseLibrary(["/a"], "/b", true), ["/a", "/b"]);
  assert.deepEqual(chooseLibrary(["/a", "/b"], "/a", true), ["/b"]);
  assert.deepEqual(chooseLibrary(["/a", "/b"], null, false), []);
  assert.deepEqual(chooseLibrary(["/a", "/b"], null, true), []);
});

test("counts of the views are within the library", () => {
  const lists: Lists = { recent: ["a1", "b1"], added: ["b2", "a2", "a1"], duplicates: [["a2", "b2"]] };
  const counts = (bibs: string[]) => viewCounts(rows, lists, bibs);
  assert.deepEqual(counts([]), { all: 4, recent: 2, added: 3, nopdf: 2, broken: 1, duplicates: 2 });
  assert.deepEqual(counts(["/x/a.bib"]), { all: 2, recent: 1, added: 2, nopdf: 1, broken: 1, duplicates: 1 });
  assert.deepEqual(counts(["/x/a.bib", "/x/b.bib"]), counts([]));
});

test("tag counts as the core counts them", () => {
  const t = (list: Row[]) => tagCounts(list).map((c) => [c.tag, c.count, c.under]);
  // "AMOC" and "climate/amoc" are two tags; the parent climate no paper of a has is counted from the one under it
  assert.deepEqual(t(rows), [
    ["AMOC", 1, 1],
    ["climate", 1, 2],
    ["climate/amoc", 1, 1],
    ["ice", 1, 1],
  ]);
  assert.deepEqual(t(rows.filter((r) => r.bib === "/x/a.bib")), [
    ["AMOC", 1, 1],
    ["climate", 0, 1],
    ["climate/amoc", 1, 1],
    ["ice", 1, 1],
  ]);
  // spellings are one tag, named as written first
  assert.deepEqual(t([row("x", "b", ["Amoc"]), row("y", "b", ["amoc", "AMOC"])]), [["Amoc", 2, 2]]);
  // the tags under a parent come together, before "a-b"
  assert.deepEqual(t([row("x", "b", ["a-b", "a/c", "a/b"])]).map((c) => c[0]), ["a", "a/b", "a/c", "a-b"]);
});

test("tag filters toggle, as alternatives with ⌘, and follow a rename", () => {
  assert.deepEqual(toggleTag([], "amoc"), [["amoc"]]);
  assert.deepEqual(toggleTag([["amoc"], ["ice"]], "AMOC"), [["ice"]]);
  assert.deepEqual(toggleTag([["amoc"]], "ice"), [["amoc"], ["ice"]]);
  // ⌘: an alternative to the last group, a group of its own without one; out again
  assert.deepEqual(toggleTag([], "ice", true), [["ice"]]);
  assert.deepEqual(toggleTag([["x"], ["amoc"]], "ice", true), [["x"], ["amoc", "ice"]]);
  assert.deepEqual(toggleTag([["x"], ["amoc", "ice"]], "Ice", true), [["x"], ["amoc"]]);
  assert.deepEqual(toggleTag([["x"], ["amoc", "ice"]], "x", false), [["amoc", "ice"]]);
  assert.ok(hasTag([["a"], ["Climate/AMOC"]], "climate/amoc"));
  assert.ok(!hasTag([["a"]], "b"));
  assert.deepEqual(renameTags([["climate"], ["climate/amoc", "ice"]], "climate", "weather"), [["weather"], ["weather/amoc", "ice"]]);
  assert.deepEqual(renameTags([["amoc", "AMOC2"]], "amoc2", "AMOC"), [["amoc"]]);
  // two groups that become the same: once
  assert.deepEqual(renameTags([["amoc"], ["AMOC2"], ["x", "y"]], "amoc2", "AMOC"), [["amoc"], ["x", "y"]]);
});

test("chips, the label of the tab, and taking a filter off", () => {
  assert.deepEqual(chips(NO_FILTERS), []);
  assert.equal(tabFilters(NO_FILTERS), "");
  const f: Filters = { bibs: ["/x/papers.bib"], view: "nopdf", tags: [["amoc"]], query: " ice " };
  assert.deepEqual(
    chips(f).map((c) => c.text),
    ["papers.bib", "No PDF", "amoc", '"ice"'],
  );
  assert.equal(tabFilters(f), 'papers.bib · No PDF · amoc · "ice"');
  assert.equal(tabFilters({ ...NO_FILTERS, query: "amoc" }), '"amoc"');
  const [bib, view, tag, search] = chips(f);
  assert.deepEqual(without(f, bib), { ...f, bibs: [] });
  assert.deepEqual(without(f, view), { ...f, view: "all" });
  assert.deepEqual(without(f, tag), { ...f, tags: [] });
  assert.deepEqual(without(f, search), { ...f, query: "" });
  // several libraries, a group of alternatives: one chip each, with all of them
  const g: Filters = { ...NO_FILTERS, bibs: ["/x/a.bib", "/y/b.bib"], tags: [["x"], ["amoc", "ice"]] };
  assert.equal(tabFilters(g), "Library: a.bib, b.bib · x · Tag: amoc, ice");
  assert.deepEqual(without(g, chips(g)[2]), { ...g, tags: [["x"]] });
});

test("filters restored from what was saved", () => {
  assert.deepEqual(restored(undefined), NO_FILTERS);
  assert.deepEqual(restored({ bibs: ["/x/a.bib", "/x/b.bib"], view: "broken", tags: [["a", "b"], ["c"]], query: "q" }, ["/x/a.bib", "/x/b.bib"]), {
    bibs: ["/x/a.bib", "/x/b.bib"],
    view: "broken",
    tags: [["a", "b"], ["c"]],
    query: "q",
  });
  // of an older version: one library, a tag per filter
  assert.deepEqual(restored({ bib: "/x/a.bib", tags: ["a", "b"] } as never, ["/x/a.bib"]), {
    ...NO_FILTERS,
    bibs: ["/x/a.bib"],
    tags: [["a"], ["b"]],
  });
  // a .bib no longer there, a view not known, a wrong type, an empty group
  assert.deepEqual(restored({ bib: "/x/gone.bib", view: "nope" as never, tags: "a" as never } as never, ["/x/a.bib"]), NO_FILTERS);
  assert.deepEqual(restored({ bibs: ["/x/gone.bib", 3 as never], tags: [[], [2 as never]] }, ["/x/a.bib"]), NO_FILTERS);
});

test("a filter of any of several values, quoted as the core reads them", () => {
  assert.equal(anyOf("src", ["tc", "Nature Geosci"]), 'src:tc,"Nature Geosci"');
  assert.equal(anyOf("src", ["The Cryosphere", "Nature, Geosci"]), 'src:"The Cryosphere","Nature, Geosci"');
  assert.equal(anyOf("tag", ["amoc", "ice*", "climate/amoc"]), "tag:amoc,ice*,climate/amoc");
  assert.equal(anyOf("prof", ['say "hi"', "", "  "]), 'prof:"say hi"');
  assert.equal(anyOf("kind", []), "");
  assert.equal(tagTerm("mass balance"), 'tag:"mass balance"');
  assert.equal(tagTerm("a,b"), 'tag:"a,b"');
  assert.deepEqual(alternatives('tc,"Nature, Geosci",'), ["tc", "Nature, Geosci", ""]);
  assert.deepEqual(alternatives('"mass balance"'), ["mass balance"]);
});

test("a typed tag filter follows a rename, any of its values", () => {
  assert.equal(renameTagFilter("ice tag:amoc -tag:x", "amoc", "AMOC"), "ice tag:AMOC -tag:x");
  assert.equal(renameTagFilter('tag:x,"climate/amoc",ice', "climate", "clim"), "tag:x,clim/amoc,ice");
  assert.equal(renameTagFilter("tag:mb,ice", "mb", "mass balance"), 'tag:"mass balance",ice');
  assert.equal(renameTagFilter('tag:"a b",c', "x", "y"), 'tag:"a b",c');
});
