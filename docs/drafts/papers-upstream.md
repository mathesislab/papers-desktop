Bugs and questions for papers (perrette/papers, v3), found while porting it
(papers-rs) and testing the app against 3.0.0b5.dev8. To send once
papers-rs works; alongside the open questions of `design/` (branch
`design-spec`).

## Bugs

- `add --on-duplicate skip --porcelain`: the skipped entry has no
  `existing KEY` line (`_insert_entry_check` returns `[]` instead of
  `[existing]`), and the `skipped: …` message is the only trace.
- `distinct.json` crashes papers: `{"distinct": 5}`, `{"distinct": null}`
  (TypeError); `{"distinct": [["a","b"]]}` or `[[1,2]]` (AttributeError in
  `same_paper`).
- Name/key templates crash with a traceback: `{journal:>5}` on an entry
  without journal (TypeError, None with a format spec); `{year.real}`,
  `{year[x]}` (not caught as TemplateError).
- `Biblio.move` (bib.py:193-197) records a `moved` operation when the user
  declined "Replace?": undo would move a file that never moved.
- python-slugify: one decimal reference above U+10FFFF cancels all decimal
  decoding of the text (its try/except wraps the whole substitution).
- `edit --set key=zzz` writes a literal `key` field.
- `status --bibtex FILE` ignores the option.
- A merge turns `month = Mar` (a macro) into `month = {Mar}` (also in
  design/ open questions, (a) 9).
- `undo`, `redo`, `restore` with nothing to do still create the library id
  and a start state (history.md 5.3 says no trace).
- `restore`: a missing piece in `hashes(n)` escapes as a traceback, not the
  "history is damaged" message.
- `history --remove` of a place without head still prints "removed the
  history of …".
- `tag --rename --porcelain` prints no line (formats.md 7.2 lists
  `modified`).
- A refused `file rename` (path safety, unknown placeholder) prints
  `papers: error:` but exits 0.
- A CSL-JSON author given as a plain string is split into one name per
  character (formats.md 2.3: a literal name).
- `add` of a record without key, author, year or DOI writes `@misc{,`.
- `edit --key ''` opens the editor (hangs without a terminal).
- A rename that changes only the case of a key is reported as removed plus
  added; `crossref` is not updated.

## Questions

- Search matches the raw bibtex: `Muller` does not find `M{\"u}ller` or
  `Müller`.
- `history --all` lists the projects by their random id: the order is not
  stable.
- bibtexparser's splitter warnings are printed twice.
- Transliteration (design/ (c) 1): Unidecode is GPL, text-unidecode
  Artistic; papers-rs uses deunicode (BSD). A table with a permissive
  licence pinned by the standard? 36 096 code points differ between
  deunicode and Unidecode 1.4.0 (Latin letters agree).
- Spec vs code (code taken as truth): slugify decodes only the HTML 4
  entities; `{doi_or_id}` with an empty `doi`; the extension is `splitext`'s
  (`.hidden` has none); tag patterns strip all trailing `/`; ISBN strips all
  leading `:`; `\d`/`isdigit` are Unicode; `parse_name` does not split a
  name with a newline; `distinct.json` that is JSON but not an object;
  the "damaged" message quotes the KeyError (`'piece X is missing…'`); a
  bibtex without library with `history-separate` uses a separate store
  (2.6 says the shared one).
- The order of `tag` listing two tags that differ only by case is not
  reproducible.
- Is the default line format of `list` a contract (formats.md 6: "should
  match")?
