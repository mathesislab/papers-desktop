# papers-rs

A Rust port of [papers-cli](https://github.com/perrette/papers). papers-cli is
the reference: papers-rs does what it does, with the same output and the same
files, so that both can work on the same library. The conformance suite
(`spec/`) checks it.

- `papers-rs`: the library.
- `papers-rs-cli`: the command line, `papers-rs`.

## Modules

- `config`: the library configuration (`.papers/config.json`), the global
  configuration (the default library, the default settings), the settings with
  their sources (library, else global, else default), the library of a
  directory (`-C DIR`, upward search, the default library), and the files
  written as papers writes them (key order, indentation, no trailing newline).
  Paths are strings with Python's `os.path` semantics (`config::paths`).
- `bib`: the bibtex of a library, read and written as papers does (bibtexparser
  v2 with the middlewares of papers), byte for byte; LaTeX and unicode as papers
  converts them: by the tables of `papers.latexenc` (`bib::latex_table`) and by
  pylatexenc (`bib::unicode`: the decoding of a library or an entry, and of the
  values for the export; the `latexenc-rs` crate); names, keywords and the `file`
  field (`bib::encoding`). Not yet: the export formats themselves (CSL-JSON, RIS)
  and the name parsing of `papers.formats`.
- `entry`: what papers knows of the fields of an entry: the `file` links
  (relative to the bibtex directory, or absolute), the tags of `keywords`
  (nested with `/`: `tag:x` finds `x` and the tags under it, counts of a subtree,
  rename of a subtree), the former keys of `ids`.
- `names`: the keys and the names of the files: the key and name templates
  (Python's `str.format`), slugify, path safety, the suffix rule of the keys,
  the names of the other files of an entry. The transliteration to ASCII is one
  function, `names::ascii`: papers uses Unidecode, papers-rs the `deunicode`
  crate, which differs for some characters (below).
- `identity`: when two entries are the same paper (DOI, arXiv id, ISBN, authors
  and title; the title guard), `.papers/distinct.json`, the search for
  duplicates, naming an entry by key, former key, DOI, arXiv id or ISBN.
- `merge`: the two-way merge of entries that are the same paper, and what
  `--on-duplicate` keep, update, skip and error do (the dialogue of `ask` is for
  the program that asks: it implements `merge::Resolve`).
- `library`: a bibtex with its entries and configuration, as papers' `Biblio`:
  load, insert (with the duplicate check, the key template, the files placed by
  the name template), remove, rename keys, set files and tags, save (an unchanged
  library is not written, nor a text that is on disk; atomic; a bibtex that
  another program wrote is refused). `Library::save` returns what the history
  records (the text before and after, the file operations, the renamed keys); the
  history itself is not in this module.
- `history`: the history store that papers, papers-rs and the app share (objects,
  states, heads, retention), recording a step, undo, redo, restore, the file
  operations, `.papers/files.json`, the lines of `papers history`; not yet
  `history --revert` (it needs the merge). `history::Step` is what the library
  model hands over after a save.
- `install`: what `papers init` creates.
- `lock`: the lock of a library (`.papers/lock`), a lease with a refresh thread.
- `error`, `log`: papers' errors, and its messages (`papers: warning: ...`).

## The command line

`papers-rs` (the `papers-rs-cli` crate) has the commands, options and errors of
papers. `init` and `config` are ported; the other commands print
`papers: error: not implemented in papers-rs yet` (status 1).

Tests: `cargo test -p papers-rs -p papers-rs-cli`. The history is checked against
recordings of papers (`crates/papers-rs/tests/history/*.json`, made by
`tests/gen_history.py` with the reference python), and, with
`PAPERS_REFERENCE=papers cargo test -p papers-rs --test history -- --ignored`, by
running papers on the store that papers-rs wrote. The cases in
`crates/papers-rs-cli/tests/fixtures/cases` were recorded by running papers-cli
(`tests/record/record_cases.py`), and the help of the commands by
`tests/record/record_help.py` (both run with a `papers` executable; they use
temporary home directories).

The library model (`names`, `entry`, `identity`, `merge`, `library`) is tested
against papers-cli: `tests/gen_library_expected.py` runs the reference (names, keys,
file names, verdicts, merges, sequences of operations on libraries, files placed
on a tree) and records `tests/expected/library/*.json`; `tests/names.rs`,
`identity.rs`, `library.rs` replay them (`papers_unit.rs` has the unit tests of
papers, ported). Python with papers installed: `python3
tests/gen_library_expected.py`.

The transliteration of papers is Unidecode (Python); papers-rs uses `deunicode`
behind `names::ascii`, until the standard pins a table. Where they differ
(`tests/expected/library/translit.json`: below U+3000 one by one, above in ranges,
mostly CJK and the other large scripts; the Latin letters that names are made of
agree) a differing result is counted as a known difference by the tests
(`tests/common/translit.rs`), not a failure, and a change of the table of
`deunicode` fails `the_transliteration_differences_are_the_recorded_ones`.
