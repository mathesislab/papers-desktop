# papers-rs

A Rust port of [papers-cli](https://github.com/perrette/papers). papers-cli is
the reference: papers-rs does what it does, with the same output and the same
files, so that both can work on the same library. The conformance suite
(`design/cases` of papers, in `spec/papers`) checks it.

- `papers-rs`: the library.
- `papers-rs-cli`: the command line, `papers-rs`.

papers-rs is 3 to 7 times faster than papers on a library of 2,346 entries: see
[PERFORMANCE.md](PERFORMANCE.md).

## Modules

- `config`: the library configuration (`.papers/config.json`), the global
  configuration (the default library, the default settings), the settings with
  their sources (library, else global, else default), the library of a
  directory (`-C DIR`, upward search, the default library), and the files
  written as papers writes them (key order, indentation, no trailing newline; atomically). A
  setting that a library was given is saved, also with the default value (`Config::is_set`); the
  keys that papers does not know are kept when it saves the file; a value of the wrong type, or a
  file (library or global) that is not UTF-8, JSON or a JSON object, is an error that names the file and the
  setting (`settings::check_settings`). All of `.papers/` is in the library folder, also when the
  bibtex is in another one (`Config::libdir`): what earlier builds wrote next to the bibtex is moved
  there when the configuration is loaded (`Config::move_papers_files`; not with a dry run:
  `config::dry_run`). The program loads the library as in a dry run (`config::as_dry_run`), and moves the files once
  its command line is read and is not one (`--dry-run`, an abbreviation, `file mv -n`: `config::set_dry_run`; a command
  line with an error moves nothing). `~` is the home directory in `bibtex`, `filesdir`, `default` and `history-dir`; a relative
  `default` or `history-dir` is relative to the folder of the global configuration
  (`MachineConfig::absolute`). Paths are strings with Python's `os.path` semantics (`config::paths`).
- `bib`: the bibtex of a library, read and written as papers does (bibtexparser
  v2 with the middlewares of papers), byte for byte, as UTF-8 with LF line ends and
  without a byte order mark (field names and keys ignore case: a second one is a
  block that cannot be read); LaTeX and unicode as papers converts them: LaTeX as
  LaTeX reads it, for the same-paper rule and the Mendeley links (`bib::latex`),
  unicode as LaTeX by the character table of papers (`data/latex-characters.tsv`, generated
  into Rust by `tools/gen_latexenc.py`; `bib::latex_table`), and
  LaTeX as unicode by pylatexenc (`bib::unicode`: the decoding of a library or an
  entry, and of the values for the export; the `latexenc-rs` crate); names (split at
  brace depth 0), keywords and the `file` field (`bib::encoding`).
- `formats`: the export formats (`papers.formats`, spec `formats.md`): CSL-JSON, RIS
  (written as the `rispy` package writes it: no dependency), BibJSON, the JSON text
  they are written in, the name parsing, the dates, the pages and the DOIs they
  share, and the lines of `papers list` (colours and terminal hyperlinks included).
  The import of CSL-JSON, RIS and BibJSON texts as bibtex entries (`formats::import::loads`:
  what `add FILE` and `edit --from` read), with the errors and quirks of papers (of papers 3.0.0b6:
  `&` `%` `#` `_` escaped for LaTeX in RIS and CSL-JSON, the stray lines of RIS and a last record
  without `ER`, numbers of BibJSON as written, `bibtex:type` for a bibtex field named as a BibJSON key)
  and of the `rispy` reader. A CSL-JSON `author` that is a string or an object is one name (papers
  3.0.0b7.dev49).
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
  (Python's `str.format`), slugify (HTML references decoded first), path safety, file names
  valid on every system and of at most 250 bytes, the suffix rule of the keys (a suffix in
  either case, a second letter after `z`), the key of a new entry that the template gives
  none for (`names::fallback_key`), the names of the other files of an entry. The
  transliteration to ASCII is one function, `names::ascii`: papers' own table (AnyAscii),
  by the `any_ascii` crate (below).
- `identity`: when two entries are the same paper (DOI, arXiv id, ISBN, authors
  and title; the title guard), `.papers/distinct.json`, the search for
  duplicates, naming an entry by key, former key, DOI, arXiv id or ISBN. A `distinct.json` whose
  `distinct` is not an array is unreadable (one warning); a pair that is not two objects is ignored.
- `merge`: the merge of entries that are the same paper, and what `--on-duplicate` keep, update,
  skip and error do (the dialogue of `ask` is for the program that asks: it implements
  `merge::Resolve`). Two-way (`Merge::new`: `add`, `check`) and three-way (`Merge::exchange`, with a
  base: what one side changed since the base wins, a field changed on both sides is a conflict, a
  removal is a change): the merge of `pull` and `push` (the contexts `Pull`, `Push`; `keep` and `update`
  name the first and the second entry, so the sides swap between the two) and of a revert (`Revert`).
  The module documentation has the rules and the modes.
- `library`: a bibtex with its entries and configuration, as papers' `Biblio`:
  load (`distinct.json` of the library folder, `Config::libdir`; absolute links of a library with
  relative paths written relative), insert (with the duplicate check, the key template, the files placed by
  the name template), remove, rename keys, set files and tags, save (an unchanged
  library is not written, nor a text that is on disk; atomic; a bibtex that
  another program wrote is refused). `Library::save` returns what the history
  records (the text before and after, the file operations, the renamed keys); the
  history itself is not in this module.
- `history`: the history store that papers, papers-rs and the app share (objects,
  states named by a hash of the state, heads, retention; the numbered states of papers 3.0.0b5 are
  upgraded when a project is opened), recording a step, undo, redo, restore (on `History::for_travel`: with nothing
  to do, they write no identifier, no state, no store folder; a change made outside papers counts as a step to undo),
  the file operations, `.papers/files.json`, the lines of `papers history` (`--all` in the order of the bibtex of the
  place last used) and the removal of a history (nothing is removed with a dry run). A pool that moved on since the
  step that undo or redo takes back is *reverted* there (a new step of the pool, `reverts`; for a
  redo, a step the undo took back is done again), by the revert below; `Moved::links` holds the new
  pool steps, for the auto-push that follows (a push after an undo is a new step: nothing is left to
  redo). A damaged file of the store (a piece empty, cut short or not UTF-8, a file object that
  does not have its checksum, a state file that is not JSON: `Project::damaged`) stops the command
  that needs it before anything changes, naming the step (`History::plan` reads first what undo,
  redo and restore need); `papers history` marks it. `history::Step`
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
- `pools`: the pools of a library (spec `exchange.md`). `pools::origin`: `.papers/origin/` (of the library folder), the records
  of what was exchanged with each pool (the base of the next three-way merge, without `ids`), the pool ids (the
  identifier of the pool's library; the records under a path id move to it, but not with `status`) and the names of
  their files (after the pool folder, `papers-pool-path`; the files of one id are merged, the others removed at the
  save), the legacy `origin.bib`; `pools::pool`: the pool of a library and the libraries given by `--pool PATH`,
  loaded when first used and locked with the library; `pools::exchange`: `Exchange`, the pool entry of a library
  entry (and the other way round), `pull`, `push`, the links of a shallow library, the repair of broken links to
  pool files, with the merge of `merge`; `pools::lookup`: `add` looks in the pool first; `pools::state`: the pool
  line of `papers status` (it writes nothing in the library, repairs links in memory, sees entry types and entries removed
  from the pool) and `exchanges.json` (the pieces of the bibtex texts, so that only what changed is compared).
- `command::pool`: what a writing command does with the pool of a shallow library: the broken links repaired
  when it loads the library, the files placed in the pool (`Session::use_pool_files`), the **automatic push** at
  `Session::commit` (new entries always, changed ones with `auto-push`, the links that changed; conflicts are
  skipped, the notes are in `Committed::notes`), the pools saved as steps of their own histories linked to the
  step of the library (`Committed::link`, `Step::pools`), then the origin records, then the library;
  `push_after_travel` for undo and redo; the conversions of `papers config pool` (`plan_own_files`,
  `plan_relink`, `relink`). `command::exchange`: `pull`, `push` and `sync`, operations on a `Session` that
  say what they did to its `Say` and take the questions of a conflict from a `Resolve`.
- `command::revert`: `papers history --revert STEP` (`Session::revert`): the changes of step STEP taken back
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
- `lock`: the lock of a library (`.papers/lock` of the library folder), a lease: its holder touches the
  file (a thread; it is never rewritten), a stale lock is taken over in one step (a rename), and the
  `.papers/.gitignore` is written when the lock is taken.
- `error`, `log`: papers' errors, and its messages (`papers: warning: ...`).

## The command line

`papers-rs` (the `papers-rs-cli` crate) has the commands, options and errors of
papers. `init`, `config`, `add`, `edit`, `rm`, `tag`, `file` (and the deprecated `filecheck`), `list`,
`export`, `check`, `migrate`, `pull`, `push`, `sync`, `history` (with `--revert`), `undo`, `redo` and `restore` are ported;
`open` and the viewer passthrough print `papers: error: not implemented in papers-rs yet` (status 1). Of `list`, the
selection and the outputs are there, and `--export`, and all the actions (on the files, `--delete`, `--edit`,
`--add-tag`, `--rm-tag`, `--fetch`, `--push`, `--pull`, `--sync`).

Following papers 3.0.0b7.dev49 (perrette/papers#188, #191), as to the commands: a refused
`file rename`, `file attach`, `list --rename` and `list --attach` (a name outside the files directory,
an unknown or invalid placeholder: the other entries are still placed and saved) and `status --bibtex FILE`
for a FILE that is missing exit with status 1; `tag --rename` prints `modified KEY` with `--porcelain`, a key
changed in case only is `renamed OLD NEW` (and `crossref` follows), a duplicate skipped by
`add --on-duplicate skip` is `existing KEY`; `edit --key ''` and `edit --set key=...` (in any case) are
refused; `papers tag` lists by count, then lower case, then the tag; the words of `list` compare the
`norm` of both sides (`Muller` finds `M{\"u}ller`); the files of `.papers/` next to the bibtex are moved
once the arguments say the command is not a dry run (`--dry`, `file mv -n`). `tests/fixes_cmds.rs` has
the tests ported from papers, and runs papers on the same cases with `PAPERS_REFERENCE`. Not done: the
warnings of bibtexparser's splitter (`bibtexparser.splitter: warning: ...`, once since this version)
are not printed.

Tests: `cargo test -p papers-rs -p papers-rs-cli`. The history is checked against
recordings of papers (`crates/papers-rs/tests/history/*.json`, made by
`tests/gen_history.py` with the reference python), and, with
`PAPERS_REFERENCE=papers cargo test -p papers-rs --test history -- --ignored`, by
running papers on the store that papers-rs wrote. A step is named by the hash of its state (it
depends on the time), so the tests compare stores and outputs with the states as `S1`, `S2`, ... in
the order of their times, and the recorded arguments name a step by its place (`$STEP:3`). The
numbered stores of papers 3.0.0b5 are recorded too (`numbered*`), and `tests/history_spec.rs` has the
rules of the specification on the API. The cases in
`crates/papers-rs-cli/tests/fixtures/cases` were recorded by running papers-cli
(`tests/record/record_cases.py`), and the help of the commands by
`tests/record/record_help.py` (both run with a `papers` executable; they use
temporary home directories). The help and the usage cases of `add`, `extract` and `fetch` have the
`--openalex` of papers #205 (df2d81d): recorded with papers ed8b4dc with the change of #205 to its
`papers/cli/parser.py` applied (`git show df2d81d -- papers/cli/parser.py | patch -p1` in a copy),
until they are recorded again from a papers that has it.

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

The transliteration of papers is AnyAscii 0.3.3 (ISC), published as a table of the
specification (`design/data/ascii-transliteration.tsv`, with the LaTeX character table
`latex-characters.tsv`): papers-rs uses the `any_ascii` crate (the same version, ISC) behind
`names::ascii`, and matches papers for every code point. `tests/tables.rs` checks it against
the copies of the two tables in `data/` (`names::ascii` for all of Unicode, `string_to_latex`
and the decoding of the accented spellings for the LaTeX table); a newer version of the crate
whose data differs fails it. To follow a newer papers, copy the two tables again (see
`tools/gen_latexenc.py`).

## Fetching and extracting

`papers-rs fetch`, `extract` and the deprecated `doi` print metadata and write no library. The
modules:

- `extract::doi`: the DOIs of a text, papers' parser character by character (`parse_dois`: the
  normalization of the text, the DOI regexps with Python's `\w`, `\d`, `\s` and the lookbehind
  written out, the choice of the article's DOI among the citations, the cleaning of what follows
  a DOI), the arXiv fallback, the DOI of the XMP (as it is) and of the document information of a PDF
  (cleaned as a DOI of the text), the
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
  the address of its user). Not ported: Google Scholar (`--scholar` says so). As of papers 3.0.0b7.dev49
  (ed8b4dc, issue 189): a [`FetchError`](src/fetch/mod.rs) of kind `Network` is papers' `OSError`
  (no answer, an HTTP error status); `fetch` and `add --doi` report it as `<doi>: <reason>`, a DOI that
  crossref and doi.org do not know and the other `DOIRequestError`s with their own message, a single file of
  `extract` that fails (not a PDF, the network) as `<file>: <reason>`, all as `papers: error: ...`, status 1
  (the reason of a network error is that of the HTTP client, not of Python's requests). A record without
  title, container title or ISBN, an `institution` that is a list and a date without year (the next of the
  published-print, published-online and issued dates is used, `0000` if none) are read as papers reads them;
  invalid JSON from doi.org is a DOI without metadata (the website is tried); the relative `.bib` links of a
  journal page are relative to the page that doi.org led to.
- `fetch::openalex`: `--openalex` of `add`, `extract` and `fetch` (papers v3 faeaf4d, #205: `papers.openalex`
  up to `fetch_bibtex_by_fulltext_openalex`), a full-text search on [OpenAlex](https://openalex.org) where
  papers has `--scholar`: the semantic search of the query (cut at 1500 characters), one request a second,
  the works scored as crossref records, the first whose title (20 letters or digits at least) is in the
  text, published works before their copies; the entry of the DOI, else made of the OpenAlex record (key
  `W...`). The answers are cached as papers caches them (`openalex/`, the same keys and content). The command
  line reads `PAPERS_MAILTO` and `OPENALEX_API_KEY` as papers does (`OpenAlex::from_env`); the library takes
  them as a parameter (`fetch::openalex::OpenAlex`, `extract::Options::openalex`, `AddRequest::openalex`), so
  that a program gives its own; they are never in a message. `tests/openalex.rs` has the tests of papers'
  `tests/test_openalex.py` and recorded answers (no network); `examples/measure_openalex.rs` measures what
  it adds on a set of PDFs.
- `fetch::html::meta_content` and `page_doi`: **extensions of papers-rs, not in papers**: the `<meta>`
  tags that journals put in the page of an article (`citation_doi`, `prism.doi`, `dc.identifier`), and the DOI they give; and
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
- `tests/fetch_unit.rs`: the unit tests of papers for these answers (`test_extract_unit.py`,
  `test_fetch_cmds.py`: the info fields of a PDF, empty titles, the institution, dates, invalid JSON, links
  of the website, the errors of `add --doi`), with answers made in the test.
- `tests/record_network.py` recorded eight real answers (crossref, doi.org, arXiv) once, into
  `tests/fixtures/network.json` (the list of references of the crossref record removed).
- `design/cases/fetch`, `design/cases/extract`: cases for the conformance suite; those that need the
  network are marked `requires: network` (`design/cases/run.py --with network`).
- `crates/papers-rs-cli/tests/fetch_extract.rs`: the command where it needs no network (a network error is
  made with a proxy that refuses the connection); `cargo test -- --ignored` has two that ask crossref.

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
are the cases `file/*`, `add/pdf-*`, `add/folder*`, `add/*doi*`, `list/file-actions` of `design/cases` (those
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
  `design/cases/history/revert-*` and `undo-pool-moved-on` are the cases recorded from papers, and
  `crates/papers-rs-cli/tests/revert_interop.rs` (with `PAPERS_REFERENCE`) has papers and papers-rs
  revert, undo and redo each other's steps, and a library with a pool (built by papers) whose pool moved
  on: `undo` and `redo` by both leave the same libraries and history, and print the same.
