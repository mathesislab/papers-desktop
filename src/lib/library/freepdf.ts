// The free PDF that the sheet adding a paper found: what it is and where it came from. A pure
// function, tested by scripts/freepdf.test.ts (npm test).

import type { FreePdf } from "../types.ts";

const VERSIONS = {
  published: "Published version",
  accepted: "Accepted manuscript",
  submitted: "Preprint",
};

/** What a free PDF found is, and the site it came from: `Accepted manuscript, from repo.example.edu`. */
export function origin(found: Extract<FreePdf, { kind: "found" }>): string {
  return `${VERSIONS[found.version]}, from ${found.site}`;
}
