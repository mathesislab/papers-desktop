// The years of the panel and the tags of the library's search, edited in a query as the core reads it.
// node --test scripts/ (npm test)

import assert from "node:assert/strict";
import { test } from "node:test";
import { clearYears, extendYears, onlyYears, toggleExcluded, toggleYear } from "../src/lib/query.ts";

test("a year clicked: that year only, or no year filter when it was so already", () => {
  assert.equal(toggleYear("ice sheet", 2012), "ice sheet y:2012");
  assert.equal(toggleYear("ice y:2012 sheet", 2012), "ice sheet");
  assert.equal(toggleYear("ice year:2010-2015", 2012), "ice y:2012");
  // an excluded year stays
  assert.equal(toggleYear("ice -y:2011", 2012), "ice -y:2011 y:2012");
});

test("a range: kept in place of the kept filters, stretched by ⇧", () => {
  assert.equal(onlyYears("a y:2001 b year:2003", 2010, 2005), "a b y:2005-2010");
  assert.equal(onlyYears("a", 2005), "a y:2005");
  assert.equal(extendYears("a y:2010", 2014), "a y:2010-2014");
  assert.equal(extendYears("a y:2010-2014", 2006), "a y:2006-2014");
  // no single kept range: the year alone
  assert.equal(extendYears("a y:2010,2012", 2014), "a y:2014");
});

test("a year excluded, or taken back; every year again", () => {
  assert.equal(toggleExcluded("ice", 2012), "ice -y:2012");
  assert.equal(toggleExcluded("ice -y:2012 sheet", 2012), "ice sheet");
  assert.equal(toggleExcluded("ice -YEAR:2012", 2012), "ice");
  assert.equal(clearYears("ice y:2010-2015 -y:2012 sheet"), "ice sheet");
});

test("a phrase is no filter: its words stay as typed", () => {
  // the regexes of before read the y: inside the quotes
  assert.equal(toggleYear('"y:2012 data" ice', 2012), '"y:2012 data" ice y:2012');
  assert.equal(clearYears('"from y:2010  to y:2015"'), '"from y:2010  to y:2015"');
  assert.equal(toggleExcluded('"-y:2012" ice', 2012), '"-y:2012" ice -y:2012');
  assert.equal(onlyYears('t:"y:1999" y:2001', 2005), 't:"y:1999" y:2005');
});
