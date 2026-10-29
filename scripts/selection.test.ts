// What can be done with the papers selected: node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  available,
  count,
  deletedMessage,
  menuTags,
  nextAfter,
  openedMessage,
  targetsOf,
  transferMessage,
} from "../src/lib/library/selection.ts";
import type { Row } from "../src/lib/types.ts";

const row = (id: string, bib = "/x/a.bib", file: Row["file"] = "found", doi: string | null = null): Row =>
  ({ id, key: id, kind: "article", authors: "A", year: 2000, title: id, container: null, tags: [], doi, file, bib }) as Row;

test("the actions that apply", () => {
  assert.deepEqual(available([row("a", "/x/a.bib", "none")]), { pdf: false, doi: false, one: true, addPdf: true });
  assert.deepEqual(available([row("a", "/x/a.bib", "found", "10.1/x")]), { pdf: true, doi: true, one: true, addPdf: false });
  // a file that is missing is not a PDF to open, and a paper with one cannot have another added here
  assert.deepEqual(available([row("a", "/x/a.bib", "missing")]), { pdf: false, doi: false, one: true, addPdf: false });
  // several: the PDFs of any, no PDF to add
  const several = available([row("a", "/x/a.bib", "none"), row("b", "/x/a.bib", "none"), row("c", "/x/a.bib", "found")]);
  assert.deepEqual(several, { pdf: true, doi: false, one: false, addPdf: false });
});

test("where papers can be copied or moved to", () => {
  const bibs = ["/x/a.bib", "/x/b.bib", "/x/c.bib"];
  assert.deepEqual(targetsOf([row("a")], bibs), ["/x/b.bib", "/x/c.bib"]);
  // papers of two files: either can take the other's
  assert.deepEqual(targetsOf([row("a"), row("b", "/x/b.bib")], bibs), bibs);
  // one .bib file: nowhere to go
  assert.deepEqual(targetsOf([row("a")], ["/x/a.bib"]), []);
});

test("the row selected after papers are removed", () => {
  const shown = ["a", "b", "c", "d", "e"].map((id) => row(id));
  assert.equal(nextAfter(shown, new Set(["b"])), "c");
  assert.equal(nextAfter(shown, new Set(["b", "c"])), "d");
  // the first removed counts: the next one that stays after it
  assert.equal(nextAfter(shown, new Set(["b", "d"])), "c");
  // at the end: the one before
  assert.equal(nextAfter(shown, new Set(["e"])), "d");
  assert.equal(nextAfter(shown, new Set(["d", "e"])), "c");
  assert.equal(nextAfter(shown, new Set(["a", "b", "c", "d", "e"])), null);
  assert.equal(nextAfter(shown, new Set(["z"])), null);
});

test("what is said", () => {
  assert.equal(count(1), "1 paper");
  assert.equal(count(3), "3 papers");
  assert.equal(count(2, "PDF"), "2 PDFs");
  const done = { added: ["a", "b"], existing: ["c"], removed: 0, missing: 1 };
  assert.equal(transferMessage(done, "/x/refs.bib", false), "Copied 2 papers to refs.bib, 1 already in refs.bib, 1 file not found");
  assert.equal(transferMessage({ ...done, existing: [], missing: 0, added: ["a"] }, "/x/refs.bib", true), "Moved 1 paper to refs.bib");
  assert.equal(transferMessage({ ...done, added: [], missing: 0 }, "/x/refs.bib", true), "Nothing moved, 1 already in refs.bib");
  assert.equal(openedMessage({ opened: 3, skipped: 2 }), "Opened 3 PDFs, 2 papers without a PDF skipped");
  assert.equal(openedMessage({ opened: 1, skipped: 0 }), "Opened 1 PDF");
  assert.equal(openedMessage({ opened: 0, skipped: 2 }), "No PDF to open (2 papers without one)");
  assert.equal(deletedMessage(3, 0), "Deleted 3 papers");
  assert.equal(deletedMessage(1, 1), "Deleted 1 paper and 1 file");
  assert.equal(deletedMessage(2, 3), "Deleted 2 papers and 3 files");
});

test("the tags of the context menu: pinned first, then the most used", () => {
  const tags = [
    { tag: "ice", count: 5, under: 5 },
    { tag: "climate", count: 0, under: 7 },
    { tag: "AMOC", count: 9, under: 9 },
    { tag: "ocean", count: 5, under: 5 },
    { tag: "a", count: 1, under: 1 },
  ];
  assert.deepEqual(menuTags([null, "ice", null], tags), ["ice", "AMOC", "ocean", "a"]);
  // a pinned tag is found in any spelling; a parent no paper has is not offered
  assert.deepEqual(menuTags(["amoc"], tags), ["amoc", "ice", "ocean", "a"]);
  assert.deepEqual(menuTags([], []), []);
});
