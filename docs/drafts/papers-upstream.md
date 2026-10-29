Bugs and questions found while porting papers (papers-rs) and testing the
app, checked again on papers v3 at 0941475 (3.0.0b7.dev3) and posted on
perrette/papers on 2026-10-29:

- perrette/papers#188: the code does not follow design/ (12 points:
  distinct.json crashes, tag --rename --porcelain, file rename exit code,
  CSL-JSON author string, edit --key '', case-only key rename, status
  --bibtex, parse_name and newlines, Unicode digits in ISBNs, name
  template traceback, warnings twice, tag order).
- perrette/papers#189: fetch, extract, add --doi: tracebacks and crashes
  on network answers.
- perrette/papers#190: history commands (--dry-run of --remove and
  --drop-files, -n out of range, undo with nothing to do, --remove of a
  place without head).
- perrette/papers#191: spec questions (skip and porcelain, empty keys,
  edit --set key, search, the list line format, history --all order,
  slugify references, five places where the spec is looser than the code).
- perrette/papers#192: the transliteration table (open question 20).
- perrette/papers#193: the conformance suite (spec/) and --openalex.
- perrette/papers#194: more points found bringing papers-rs up to v3
  (--dry-run moving .papers/, invalid UTF-8 config, redo -n 2, depth-0
  split, fit_name, list --fetch empty key).

Fixed in 3.0.0b6 or later, not posted: the declined "Replace?" in
Biblio.move (unreachable with the -2 suffix), month = Mar after a merge,
the traceback of restore with a missing piece, {journal} without journal.
By design (now in design/): a distinct.json that is JSON but not an object,
the "damaged" message, a bibtex without library and history-separate.
