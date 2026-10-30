// How the sheet adding a paper names the free PDF it found: node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { origin } from "../src/lib/library/freepdf.ts";

test("the version of the PDF and its site", () => {
  const found = (version: "published" | "accepted" | "submitted", site: string) =>
    origin({ kind: "found", path: "/tmp/a.pdf", version, site });
  assert.equal(found("published", "journals.example.org"), "Published version, from journals.example.org");
  assert.equal(found("accepted", "repo.example.edu"), "Accepted manuscript, from repo.example.edu");
  assert.equal(found("submitted", "arxiv.org"), "Preprint, from arxiv.org");
});
