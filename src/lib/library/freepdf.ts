// The free PDF that the sheet adding a paper found: what it is, where it came from, and the
// published version to open in a browser. Pure functions, tested by scripts/freepdf.test.ts (npm test).

import type { FreePdf } from "../types.ts";

/** the PDF of the sheet: whether the app downloaded it (papers always places a download in the library); for a free
 *  PDF found, what it is and where it came from, and the published version to open in a browser */
export type SheetPdf = { path: string; downloaded: boolean; origin?: string; published?: string | null };

const VERSIONS = {
  published: "Published version",
  accepted: "Accepted manuscript",
  submitted: "Preprint",
};

/** What a free PDF found is, and the site it came from: `Accepted manuscript, from repo.example.edu`. */
export function origin(found: Extract<FreePdf, { kind: "found" }>): string {
  return `${VERSIONS[found.version]}, from ${found.site}`;
}

/** A free PDF found, as the PDF of the sheet. */
export function sheetPdf(found: Extract<FreePdf, { kind: "found" }>): SheetPdf {
  return { path: found.path, downloaded: true, origin: origin(found), published: found.published };
}
