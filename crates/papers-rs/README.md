# papers-rs

A Rust port of [papers-cli](https://github.com/perrette/papers). papers-cli is
the reference: papers-rs does what it does, with the same output and the same
files, so that both can work on the same library. The conformance suite
(`spec/`) checks it.

- `papers-rs`: the library.
- `papers-rs-cli`: the command line, `papers-rs`.

papers-rs is 3 to 8 times faster than papers on a library of 2,344 entries: see
[PERFORMANCE.md](PERFORMANCE.md).

## Modules

- `config`: the library configuration (`.papers/config.json`), the global
  configuration (the default library, the default settings), the settings with
  their sources (library, else global, else default), the library of a
  directory (`-C DIR`, upward search, the default library), and the files
  written as papers writes them (key order, indentation, no trailing newline).
  Paths are strings with Python's `os.path` semantics (`config::paths`).
- `bib`: the bibtex of a library, read and written as papers does (bibtexparser
  v2 with the middlewares of papers), byte for byte, as UTF-8 with LF line ends and
  without a byte order mark (field names and keys ignore case: a second one is a
  block that cannot be read); LaTeX and unicode as papers converts them: LaTeX as
  LaTeX reads it, for the same-paper rule and the Mendeley links (`bib::latex`),
  unicode as LaTeX by the table of `papers.latexenc` (`bib::latex_table`), and
  LaTeX as unicode by pylatexenc (`bib::unicode`: the decoding of a library or an
  entry, and of the values for the export; the `latexenc-rs` crate); names (split at
  brace depth 0), keywords and the `file` field (`bib::encoding`).
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
  (Python's `str.format`), slugify, path safety, file names valid on every system and
  of at most 250 bytes, the suffix rule of the keys (a suffix in either case, a second
  letter after `z`), the names of the other files of an entry. The transliteration to
  ASCII is one function, `names::ascii`: papers uses Unidecode (for the slugs too, since
  3.0.0b6), papers-rs the `deunicode` crate, which differs for some characters (below).
- `identity`: when two entries are the same paper (DOI, arXiv id, ISBN, authors
  and title; the title guard), `.papers/distinct.json`, the search for
  duplicates, naming an entry by key, former key, DOI, arXiv id or ISBN.
- `merge`: the merge of entries that are the same paper, and what `--on-duplicate` keep, update,
  skip and error do (the dialogue of `ask` is for the program that asks: it implements
  `merge::Resolve`). Two-way (`Merge::new`: `add`, `check`) and three-way (`Merge::exchange`, with a
  base: what one side changed since the base wins, a field changed on both sides is a conflict, a
  removal is a change): the merge of `pull` and `push` (the contexts `Pull`, `Push`; `keep` and `update`
  name the first and the second entry, so the sides swap between the two) and of a revert (`Revert`).
  The module documentation has the rules and the modes.
- `library`: a bibtex with its entries and configuration, as papers' `Biblio`:
  load, insert (with the duplicate check, the key template, the files placed by
  the name template), remove, rename keys, set files and tags, save (an unchanged
  library is not written, nor a text that is on disk; atomic; a bibtex that
  another program wrote is refused). `Library::save` returns what the history
  records (the text before and after, the file operations, the renamed keys); the
  history itself is not in this module.
- `history`: the history store that papers, papers-rs and the app share (objects,
  states, heads, retention), recording a step, undo, redo, restore, the file
  operations, `.papers/files.json`, the lines of `papers history`. A pool that moved on since the
  step that undo or redo takes back is *reverted* there (a new step of the pool, `reverts`), by the
  revert below; `Moved::links` holds the new pool steps, for the auto-push that follows. `history::Step`
  is what the library model hands over after a save.
- `command`: the commands that write a library, and the write path they share
  (`command::session`: the lock, the history before the command and the head sync,
  the load, the atomic save refused when the bibtex changed meanwhile with the files put
  back, `.papers/files.json`, the history step with papers' command text, the file
  operations after the save, the dry run; nothing is printed there: lines go to a `Say`,
  questions to a `Prompt`). `add` (bibtex, CSL-JSON, BibJSON, RIS files; the fields given;
  `--key` with `--attach`; PDFs, folders with `-R`, `--doi`, URLs as attachments: `command::add_pdf`,
  the DOI of a PDF and the queries through `command::queries`, a download by `fetch::download`),
  `edit`, `rm`, `tag` and the `papers file` commands (`command::file_cmd`: rename, mv, attach, detach,
  rm, check, fix, clean, checksum, and the actions of `papers list` on the files) are operations on a
  `Session`; the other writing commands use the same `Session`. Not yet: Google Scholar
  (`add --scholar`).
- `pools`: the pools of a library (spec `exchange.md`). `pools::origin`: `.papers/origin/`, the records
  of what was exchanged with each pool (the base of the next three-way merge), the pool ids and the names of
  their files, the legacy `origin.bib`; `pools::pool`: the pool of a library and the libraries given by `--pool PATH`,
  loaded when first used and locked with the library; `pools::exchange`: `Exchange`, the pool entry of a library
  entry (and the other way round), `pull`, `push`, the links of a shallow library, the repair of broken links to
  pool files, with the merge of `merge`; `pools::lookup`: `add` looks in the pool first; `pools::state`: the pool
  line of `papers status` and `exchanges.json` (the pieces of the bibtex texts, so that only what changed is
  compared).
- `command::pool`: what a writing command does with the pool of a shallow library: the broken links repaired
  when it loads the library, the files placed in the pool (`Session::use_pool_files`), the **automatic push** at
  `Session::commit` (new entries always, changed ones with `auto-push`, the links that changed; conflicts are
  skipped, the notes are in `Committed::notes`), the pools saved as steps of their own histories linked to the
  step of the library (`Committed::link`, `Step::pools`), then the origin records, then the library;
  `push_after_travel` for undo and redo; the conversions of `papers config pool` (`plan_own_files`,
  `plan_relink`, `relink`). `command::exchange`: `pull`, `push` and `sync`, operations on a `Session` that
  say what they did to its `Say` and take the questions of a conflict from a `Resolve`.
- `command::revert`: `papers history --revert N` (`Session::revert`): the changes of step N taken back
  per entry by a three-way merge (base: the entry at N; incoming: the entry before N; target: the entry
  now), the file links and the tags one by one, the key, `distinct.json`, and the files as undo moves
  them, saved as a step with `reverts`. The same revert takes back a pool step in `undo` and `redo`.
- `command::check`: `papers check` (and the `list --fetch` of the CLI): the fixes of an entry
  (names, encoding, DOI, fetch, key; `fix_entry`, with the question and the diff of papers), the
  duplicates merged, the lines of what changed. A fetch goes through the `Fetch` trait (crossref in
  the program, what was recorded in the tests). `command::migrate`: `papers migrate` (the global
  configuration of v2 as a library, the `.papersconfig.json` moved, `--folders`).
- `difflib`: Python's `difflib.ndiff` (`SequenceMatcher` with junk and popular elements), for the
  diff that `check` shows.
- `install`: what `papers init` creates.
- `lock`: the lock of a library (`.papers/lock`), a lease with a refresh thread.
- `error`, `log`: papers' errors, and its messages (`papers: warning: ...`).

## The command line

`papers-rs` (the `papers-rs-cli` crate) has the commands, options and errors of
papers. `init`, `config`, `add`, `edit`, `rm`, `tag`, `file` (and the deprecated `filecheck`), `list`,
`export`, `check`, `migrate`, `pull`, `push`, `sync`, `history` (with `--revert`), `undo`, `redo` and `restore` are ported;
`open` and the viewer passthrough print `papers: error: not implemented in papers-rs yet` (status 1). Of `list`, the
selection and the outputs are there, and `--export`, and all the actions (on the files, `--delete`, `--edit`,
`--add-tag`, `--rm-tag`, `--fetch`, `--push`, `--pull`, `--sync`).

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
  that the tests replay recorded answers). The requests and their user agent are papers'
  (`Network::with_user_agent` says another one: a program that uses the library gives its own, with
  the address of its user). Not ported: Google Scholar (`--scholar` says so).
- `fetch::openalex`: **an extension of papers-rs, not in papers** (see "Extensions" below).
- `fetch::html::meta_content` and `page_doi`: **extensions too**: the `<meta>` tags that journals put
  in the page of an article (`citation_doi`, `prism.doi`, `dc.identifier`), and the DOI they give; and
  `Fetcher::openalex_free_pdf`, the link to a free PDF of a DOI that OpenAlex lists. For a program
  that adds a paper from the link of its article (Papers Desktop).

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

### Extensions

papers-rs has options that papers has not. Where one is not used, papers-rs does what papers does
(the conformance suite passes without it); its help is marked "(papers-rs)", and the help tests
expect exactly that option and nothing else (`tests/fixtures/help-openalex`).

- `--openalex` of `add`, `extract` and `fetch`: a full-text search on [OpenAlex](https://openalex.org)
  (open API, no key needed), in place of crossref's, where papers has `--scholar` (Google Scholar by
  scraping, often blocked: not ported). Same inputs as papers' chain: the query that papers makes of the
  text of a PDF (`query_text`), the result accepted only if its title is in the text. The hits of OpenAlex's
  semantic search are scored as papers scores crossref's (`token_set_ratio` of the authors, title and
  abstract), the best that has its title in the text is taken, and its DOI gives the entry by papers' own
  path (crossref, doi.org, arXiv); a hit without DOI gives an entry made of the OpenAlex work (the types,
  the names, the pages, the key `W...`). The answers are cached under `openalex/` in the cache directory.
  `PAPERS_MAILTO` (an e-mail address, the "polite pool") and `OPENALEX_API_KEY` (a free key: without it
  OpenAlex allows about 100 searches a day per IP address) are sent when set; the library takes them as
  a parameter (`fetch::openalex::OpenAlex`, `extract::Options::openalex`, `AddRequest::openalex`). The module
  documentation has the details, `tests/openalex.rs` (recorded answers, no network) the tests, and
  `examples/measure_openalex.rs` measures what it adds on a set of PDFs.

## Adding PDFs and folders, the file commands

`add` of a PDF finds its DOI as `extract` does (the PDF metadata, else the text, a DOI being accepted
only if the title of its crossref record is in the text), looks the paper up in the library first
(`command::add_pdf::find_pdf`: no query when the library has the DOI), then asks crossref; the
fields given on the command line replace those of the record, and the key is made from the key
template. A folder (`-R`) is scanned in sorted order (papers: the order of the file system); the
files `<name>--<label>.<ext>` are attachments of `<name>.pdf`. Several PDFs are looked up one by
one: papers fetches their metadata concurrently first (the same results, faster).

The `papers file` commands and the file actions of `list` are `command::file_cmd`; what a command
does with a file (`move`, `copy`, `link`, `ask`) is `command::files`, with the rules of
`library.md` (a downloaded file is always moved, a file of another library is copied). The tests
are the cases `file/*`, `add/pdf-*`, `add/folder*`, `add/*doi*`, `list/file-actions` of `spec/` (those
that need the network or a PDF say so), and `tests/command_files.rs`; `tests/interop.rs` has papers
undo what papers-rs did and the reverse (`PAPERS_REFERENCE`).

## The three-way merge and the revert

`merge::Merge::exchange(target, incoming, relative_to, base)` is the merge of `pull`, `push` and a
revert; the module documentation says what a field of it is (`Status`, `Changed`, `FieldMerge::removal`,
`decline`) and what each `--on-duplicate` mode does in each `Context`. The dialogue of `ask` is in
`papers-rs-cli` (`src/dialogue.rs`), which shows a three-way merge as papers does (where a field
changed, `(removed)`, the `origin` line, the additions in `f`).

- `tests/gen_merge3.py` makes `tests/expected/merge3.json` with the reference python: random sets of base,
  target and incoming entries (a field unchanged, changed to an equivalent spelling or to another value,
  removed, blank; the type, the keys, the former keys, the links), and for each what papers makes of it
  (the fields, the result with each side chosen, the additions declined, the screen, the questions, the
  answers, the resolver in each mode, a scripted dialogue). `tests/merge3.rs` replays the model,
  `papers-rs-cli`'s `dialogue::tests` the rest; `gen_merge3.py --count 30000 --seed 11 --out FILE` makes a
  larger set (`PAPERS_RS_MERGE3_FUZZ=FILE cargo test -p papers-rs --test merge3 fuzz_file`, and
  `cargo test -p papers-rs-cli --bin papers-rs a_larger_file`).
- `tests/gen_pools.py` makes `tests/expected/pools.json` with the reference python: the origin file of each pool id,
  the files that `Origin.save` writes for sequences of operations, and, for libraries made of a bibtex, a pool bibtex,
  origin records and `distinct.json`, the pool entry of each library entry and its record, the entries shared with
  the pool or removed from it, the library entry of each pool entry, the pool entries missing. `tests/pools.rs`
  replays them. `crates/papers-rs-cli/tests/pools_interop.rs` (with `PAPERS_REFERENCE`) has papers and papers-rs
  exchange with the same pool: what one records the other reads (the origin records, the times, `exchanges.json`),
  the same commands by both print the same and leave the same records, and one undoes what the other pushed.
- `tests/revert.rs`: the tests of papers for `history --revert` (`test_revert.py`), on the API.
  `spec/cases/history/revert-*` and `undo-pool-moved-on` are the cases recorded from papers, and
  `crates/papers-rs-cli/tests/revert_interop.rs` (with `PAPERS_REFERENCE`) has papers and papers-rs
  revert, undo and redo each other's steps, and a library with a pool (built by papers) whose pool moved
  on: `undo` and `redo` by both leave the same libraries and history, and print the same.
