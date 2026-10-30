// How the sheet adding a paper names the free PDF it found: node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { origin, sheetPdf } from "../src/lib/library/freepdf.ts";

const ARTICLE = "https://doi.org/10.1234/ice.2020.1";

type Version = "published" | "accepted" | "submitted";
const found = (version: Version, site: string, published: string | null = null) => ({
  kind: "found" as const,
  path: "/tmp/papers_dl_1/a.pdf",
  version,
  site,
  published,
});

test("the version of the PDF and its site", () => {
  assert.equal(origin(found("published", "journals.example.org")), "Published version, from journals.example.org");
  assert.equal(origin(found("accepted", "repo.example.edu", ARTICLE)), "Accepted manuscript, from repo.example.edu");
  assert.equal(origin(found("submitted", "arxiv.org", ARTICLE)), "Preprint, from arxiv.org");
});

test("the PDF of the sheet: downloaded, with the published version to open when it is another", () => {
  assert.deepEqual(sheetPdf(found("accepted", "repo.example.edu", ARTICLE)), {
    path: "/tmp/papers_dl_1/a.pdf",
    downloaded: true,
    origin: "Accepted manuscript, from repo.example.edu",
    published: ARTICLE,
  });
  assert.equal(sheetPdf(found("published", "journals.example.org")).published, null);
});
