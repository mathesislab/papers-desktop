// How the PDFs of the Downloads panel are described: node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { folderName, formatDate, formatSize, libraryNote } from "../src/lib/library/downloads.ts";

test("sizes in bytes, KB, MB", () => {
  assert.equal(formatSize(0), "0 B");
  assert.equal(formatSize(999), "999 B");
  assert.equal(formatSize(1500), "1.5 KB");
  assert.equal(formatSize(48_000), "48 KB");
  assert.equal(formatSize(1_234_567), "1.2 MB");
  assert.equal(formatSize(999_900), "1.0 MB");
  assert.equal(formatSize(25_000_000), "25 MB");
});

test("dates: the time today, the day this year, the year before", () => {
  const now = new Date(2026, 9, 29, 15, 0).getTime();
  assert.equal(formatDate(new Date(2026, 9, 29, 9, 5).getTime(), now, "en-GB"), "09:05");
  assert.equal(formatDate(new Date(2026, 9, 3, 9, 5).getTime(), now, "en-GB"), "3 Oct");
  assert.equal(formatDate(new Date(2025, 11, 31, 23, 0).getTime(), now, "en-GB"), "31 Dec 2025");
});

test("the name of a folder", () => {
  assert.equal(folderName("/Users/me/Downloads"), "Downloads");
  assert.equal(folderName("/Users/me/Downloads/"), "Downloads");
  assert.equal(folderName("C:\\Users\\me\\Inbox"), "Inbox");
  assert.equal(folderName("/"), "/");
});

test("the note of a PDF in the library: the paper and its file, a line per paper", () => {
  const a = { id: "a", key: "rahmstorf2015", file: "files/rahmstorf_2015_x.pdf", same_contents: false };
  const b = { id: "b", key: "jones2021", file: "jones.pdf", same_contents: true };
  assert.equal(libraryNote([a]), "in the library: rahmstorf2015 — files/rahmstorf_2015_x.pdf");
  assert.equal(libraryNote([b]), "in the library: jones2021 — same contents as jones.pdf");
  assert.equal(libraryNote([a, b]).split("\n").length, 2);
});
