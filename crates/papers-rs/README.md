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
  field (`bib::encoding`).
- `formats`: the export formats (`papers.formats`, spec `formats.md`): CSL-JSON, RIS
  (written as the `rispy` package writes it: no dependency), BibJSON, the JSON text
  they are written in, the name parsing, the dates, the pages and the DOIs they
  share, and the lines of `papers list` (colours and terminal hyperlinks included).
  The import of CSL-JSON, RIS and BibJSON texts as bibtex entries (`formats::import::loads`:
  what `add FILE` and `edit --from` read), with the errors and quirks of papers and of the
  `rispy` reader; the commands that read them are not ported yet.
- `search`: the selection of `papers list`: words and fields (substring, wildcards,
  fuzzy by rapidfuzz's `token_set_ratio`, which the `rapidfuzz` crate's Indel distance
  gives), tags, files, the entries to review (the DOI parser of `papers.extract`),
  the duplicates, `--by`, `--invert`.
- `export`: `papers export` (and `papers list --export`): the entries written as a new
  library, with its history (the copies of the files join a step of the library of the
  command), and in the other formats.
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
- `command`: the commands that write a library, and the write path they share
  (`command::session`: the lock, the history before the command and the head sync,
  the load, the atomic save refused when the bibtex changed meanwhile with the files put
  back, `.papers/files.json`, the history step with papers' command text, the file
  operations after the save, the dry run; nothing is printed there: lines go to a `Say`,
  questions to a `Prompt`). `add` (bibtex, CSL-JSON, BibJSON, RIS files; the fields given;
  `--key` with `--attach`), `edit`, `rm`, `tag` are operations on a `Session`; the other
  writing commands use the same `Session`. Not yet: PDFs, folders, `--doi`, URLs as
  attachments, a library with a pool (`Session::open` says so).
- `install`: what `papers init` creates.
- `lock`: the lock of a library (`.papers/lock`), a lease with a refresh thread.
- `error`, `log`: papers' errors, and its messages (`papers: warning: ...`).

## The command line

`papers-rs` (the `papers-rs-cli` crate) has the commands, options and errors of
papers. `init`, `config`, `add`, `edit`, `rm`, `tag`, `list` and `export` are ported; the other commands print
`papers: error: not implemented in papers-rs yet` (status 1). Of `list`, the
selection and the outputs are there, and `--export`; the actions that change the
library (`--delete`, `--edit`, `--add-tag`, `--rm-tag`, `--attach`, `--detach`,
`--detach-broken`, `--rename`, `--fix-files`, `--fetch`, `--push`, `--pull`, `--sync`) and
`--open` say so too.

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

The formats (`formats`, `search`) are tested against papers-cli too:
`tests/gen_formats.py` records `tests/expected/formats.json` (names, dates, pages, DOIs,
the CSL-JSON, RIS and BibJSON texts of random and hand-written entries, the scores of
rapidfuzz, `isvaliddoi`), `tests/formats.rs` replays it. Python with papers and `rispy`
installed: `python3 tests/gen_formats.py`.

The transliteration of papers is Unidecode (Python); papers-rs uses `deunicode`
behind `names::ascii`, until the standard pins a table. Where they differ
(`tests/expected/library/translit.json`: below U+3000 one by one, above in ranges,
mostly CJK and the other large scripts; the Latin letters that names are made of
agree) a differing result is counted as a known difference by the tests
(`tests/common/translit.rs`), not a failure, and a change of the table of
`deunicode` fails `the_transliteration_differences_are_the_recorded_ones`.

## Fetching and extracting

`papers-rs fetch`, `extract` and the deprecated `doi` print metadata and write no library. The
modules:

- `extract::doi`: the DOIs of a text, papers' parser character by character (`parse_dois`: the
  normalization of the text, the DOI regexps with Python's `\w`, `\d`, `\s` and the lookbehind
  written out, the choice of the article's DOI among the citations, the cleaning of what follows
  a DOI), the arXiv fallback, the DOI of the XMP and of the document information of a PDF, the
  check of a title in a text, `query_text`.
- `extract::pdf`, `extract::text`: the text of a PDF. papers uses PyMuPDF (AGPL, not usable);
  papers-rs reads the characters with `pdf-extract` (MIT, on `lopdf`) and makes the lines
  itself (a line end where the position jumps, as MuPDF does). The text is not papers' text: the
  words are mostly the same, the line ends and the order of columns are not always, and a font
  that `pdf-extract` decodes differently gives other characters. The DOI parser and the check of
  the title are papers' own, so what matters is whether the same DOIs come out:
  `tests/measure_pdfs.py` measures it on a set of PDFs (it runs papers on them).
  `pdf-extract` panics on some PDFs: papers-rs catches it (the page has no text), which needs a
  build that unwinds; with `panic = "abort"` (the release profile of this workspace) such a PDF
  ends the process.
- `extract` (mod): `extract_pdf_doi`, `extract_pdf_metadata`, `extract_txt_metadata`,
  `confirm_doi` (a DOI is taken only if its crossref title is in the text), with papers'
  messages. `--image` runs `pdftoppm` and `tesseract` as papers does.
- `fetch`: crossref (by DOI, and the bibliographic search with papers' score: `fuzz` is
  rapidfuzz's `token_set_ratio`), doi.org content negotiation (DataCite and the others), arXiv,
  the journal website (`html`), the conversion of a record to bibtex (`crossref`: the fields
  as papers chooses them, written as a library is), the cache (`cache`: the files of papers'
  `~/.cache/papers`, which both read; the old single-file caches are not migrated) and the
  transport (`http`: reqwest with rustls and ring, as the app does; a `Transport` trait, so
  that the tests replay recorded answers). The requests and their user agent are papers'.
  Not ported: Google Scholar (`--scholar` says so).

The tests (`cargo test -p papers-rs -p papers-rs-cli`, none needs the network):

- `tests/gen_extract_expected.py` runs papers (python with papers installed) and records
  `tests/expected/extract/*.json`: the DOIs of texts (the unit tests of papers and generated
  texts), the pieces, crossref records as bibtex (real ones from the cache of papers, invented
  and random ones), scores, cache files, links of HTML, and flows (by DOI, arXiv, doi.org,
  full text, a text, the PDFs of `tests/fixtures/pdf`) with the network replayed from
  fixtures: the same results and errors, and the same requests. `tests/extract.rs` and
  `tests/fetch.rs` replay them. A larger fuzz run:
  `gen_extract_expected.py --count 100000 --seed 7 --out DIR`, then
  `PAPERS_RS_EXTRACT_FUZZ=DIR/doi_text.json cargo test -p papers-rs --test extract fuzz_file`.
- `tests/record_network.py` recorded eight real answers (crossref, doi.org, arXiv) once, into
  `tests/fixtures/network.json` (the list of references of the crossref record removed).
- `spec/cases/fetch`, `spec/cases/extract`: cases for the conformance suite; those that need the
  network are marked `requires: network` (`spec/run.py --with network`).
- `crates/papers-rs-cli/tests/fetch_extract.rs`: the command where it needs no network;
  `cargo test -- --ignored` has one that asks crossref.
