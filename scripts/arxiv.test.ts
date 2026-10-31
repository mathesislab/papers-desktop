// The DOI of an arXiv id or link: node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { arxivDoi } from "../src/lib/arxiv.ts";

test("an arXiv id, with or without its version", () => {
  assert.equal(arxivDoi("2410.01234"), "10.48550/arXiv.2410.01234");
  assert.equal(arxivDoi(" 2410.01234v2 "), "10.48550/arXiv.2410.01234");
  assert.equal(arxivDoi("arXiv:2410.01234"), "10.48550/arXiv.2410.01234");
  assert.equal(arxivDoi("arXiv:hep-th/9901001"), "10.48550/arXiv.hep-th/9901001");
  assert.equal(arxivDoi("math.AG/0309136v1"), "10.48550/arXiv.math.AG/0309136");
});

test("the link of an abstract, a PDF or an HTML page", () => {
  assert.equal(arxivDoi("https://arxiv.org/abs/2610.19885"), "10.48550/arXiv.2610.19885");
  assert.equal(arxivDoi("http://www.arxiv.org/abs/2610.19885v3"), "10.48550/arXiv.2610.19885");
  assert.equal(arxivDoi("https://arxiv.org/pdf/2610.19885v1"), "10.48550/arXiv.2610.19885");
  assert.equal(arxivDoi("https://arxiv.org/pdf/2610.19885.pdf"), "10.48550/arXiv.2610.19885");
  assert.equal(arxivDoi("https://arxiv.org/html/2610.19885v2/"), "10.48550/arXiv.2610.19885");
  assert.equal(arxivDoi("https://export.arxiv.org/abs/hep-th/9901001?context=hep-th"), "10.48550/arXiv.hep-th/9901001");
});

test("the link of a mirror: ar5iv, alphaXiv", () => {
  assert.equal(arxivDoi("https://ar5iv.labs.arxiv.org/html/2310.06825"), "10.48550/arXiv.2310.06825");
  assert.equal(arxivDoi("https://ar5iv.org/abs/2310.06825v1"), "10.48550/arXiv.2310.06825");
  assert.equal(arxivDoi("https://www.alphaxiv.org/abs/2310.06825"), "10.48550/arXiv.2310.06825");
  assert.equal(arxivDoi("https://alphaxiv.org/overview/2310.06825v2"), "10.48550/arXiv.2310.06825");
});

test("not arXiv", () => {
  assert.equal(arxivDoi("10.1/x"), null);
  assert.equal(arxivDoi("https://arxiv.org/list/physics.ao-ph/new"), null);
  assert.equal(arxivDoi("https://example.org/abs/2610.19885"), null);
  assert.equal(arxivDoi("2610.198"), null);
});
