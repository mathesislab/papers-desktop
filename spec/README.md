# The papers standard: conformance suite

What an implementation of papers does, as cases: a library before, a command,
and what it prints and leaves after. papers-cli (Python) is the reference: the
cases are recorded by running it. papers-rs is checked against them.

It is the executable side of the specification that Mahé Perrette wrote for other
implementations (`design/README.md` and the files it lists, branch `design-spec` of
the papers repository): each case says which sections of it it exercises
(`sections`), and which of the outputs of each step the specification makes Contract
and which Free (see "Contract and Free"). The coverage table at the end says which
contract each case checks.

Shared by the implementations, so part of the standard: the bibtex written,
`.papers/config.json` and the global configuration, the history and the pool's
steps, the lock, the `--porcelain` lines, the keys and file names. Private to
each: caches (the parse cache).

The cases are plain files, and the two tools are Python 3 scripts with the
standard library only (`conformance.py` is shared by both):

```
python3 spec/run.py --papers papers                      # an implementation on all the cases
python3 spec/run.py --papers target/debug/papers-rs 'list/*' # on the cases that match
python3 spec/run.py --papers papers --contract-only      # only what the specification makes Contract
python3 spec/run.py --papers papers --section 'formats.md#7'  # the cases of a section
python3 spec/record.py --papers papers [CASE...]         # (re)record the expected results
```

`run.py` prints `PASS`/`FAIL` per case, and for a failure the unified diff of
the first difference (`-a`: all of them), then a summary; the exit status is 0
when all pass. By default everything is compared byte for byte. With
`--contract-only` only the Contract parts are (exit codes, the files after, the
outputs of the steps that are Contract); a difference in a Free output is
reported as a note (`PASS name (Free differences: step 1 stdout)`; `-a` shows
them) and the summary counts the cases that have some. `--section PATTERN`
(repeatable) keeps the cases that exercise a section (see "Sections").
`record.py` runs each case twice and writes the results only if the two runs
agree. `CASE` is a glob on the name of the case (`group/name`). `--list` lists
the cases with their sections, `--sections` the sections with their cases,
`--keep` keeps the sandboxes of the run.

## A case

`spec/cases/<group>/<name>/` (the groups are the commands: `init`, `config`,
`status`, `list`, `add`, `edit`, `rm`, `tag`, `file`, `export`, `history` (undo,
redo, restore), `exchange` (pull, push, auto-push), `errors`; and the topics of
the specification that no single command owns: `porcelain` (the `--porcelain`
lines of the writing commands), `bibtex` (the format of the file), `lock`):

```
case.json        what to run (written by hand)
before/          the working folder before: a tree of files (optional, empty by default)
before-home/     the HOME of the user before: config/, data/, home/ (optional)
stdin/NN.txt     the standard input of step NN, when a step has one
expected/        what the reference did (written by record.py)
    NN.stdout        standard output of step NN (absent: empty)
    NN.stderr        standard error of step NN (absent: empty)
    exit.json        the exit codes of the steps: [0, 1, ...]
    work/            the working folder after: all the files, with their contents
    home/            the HOME after (config/, data/, home/, see below)
```

`case.json`:

```json
{
  "description": "what the case shows",
  "sections": ["formats.md#7.2", "library.md#the-library-configuration/keys"],
  "ignore": ["home/data/history/*"],
  "requires": ["network"],
  "steps": [
    {"argv": ["init", "lib"]},
    {"argv": ["list", "-K"], "cwd": "lib"},
    {"argv": ["edit", "perrette2013", "--from", "-"], "stdin_file": "stdin/03.txt"},
    {"argv": ["tag", "-", "--add", "x"], "stdin": "doe2019\n"},
    {"argv": ["list", "--porcelain"], "contract": ["stderr"], "wait": 1.1}
  ]
}
```

- `steps`: the commands, run in order, each in a fresh process. `argv` is the
  arguments after the program (`papers`/`papers-rs` itself is not part of it),
  `cwd` the working folder of the step relative to `before/` (default `.`),
  `stdin_file` (a file of the case) or `stdin` (a string) the standard input;
  without either, the standard input is empty and not a terminal (`/dev/null`).
  All the steps run, whatever their exit code: the standard output, standard
  error and exit code of each are compared, then the trees after the last one.
- `requires`: capabilities the case needs (e.g. `network`, `pdf`). `run.py` and
  `record.py` skip the case unless `--with network,pdf` says they are there.
  The first set of cases needs none: it is offline, with no PDF.
- `description`: free text.
- `sections`: required, a non-empty list of the sections of the specification that the
  case exercises (see "Sections").
- `contract` (of a step): which of `"stdout"`, `"stderr"` the specification makes
  Contract for that step, replacing the default rules (see "Contract and Free"). `[]`:
  neither.
- `ignore`: globs on the files after (`work/...`, `home/...`, as in `expected/`) that the
  case does not compare. Used by the `exchange` cases, and `porcelain/config-pool`, for
  `home/data/history/*`: the origin records they leave hold times (`papers-time`), and
  the history cuts the bibtex into pieces by its content, so the pieces and the states
  that name them differ from one run to the next.
- `wait` (of a step): seconds to wait before the step. The exchanges compare times with a
  resolution of one second (without it a pull right after a push in the same second can
  miss a change; the cases of `exchange` wait 1.1 s).

Folders that are empty are kept as an empty file `.emptydir` (git does not
track folders), and `.gitignore` is stored as `.gitignore.spec` (the
`.papers/.gitignore` of a library contains `*`, which would hide its
neighbours). Both are only about storing: the tools restore the real names.

## Sections

`sections` refer to the files of the specification (`design/*.md`) by a stable
reference, `FILE.md#REF`:

- `formats.md`, `bibtex.md`, `history.md`, `exchange.md` number their sections: `REF` is
  the number of the heading, `formats.md#7`, `formats.md#7.2`, `bibtex.md#8`,
  `history.md#6.2`, `exchange.md#12`.
- `library.md` has no numbers: `REF` is the heading of the `##` section up to its colon, in
  lower case with hyphens for the spaces (`library.md#the-library-configuration`,
  `library.md#finding-the-library-of-a-command`, `library.md#the-lock`), and, for a `###`
  heading under it, `/` and the same form of that heading
  (`library.md#the-library-configuration/which-keys-papers-writes`,
  `library.md#settings/precedence`, `library.md#layout/papersgitignore`: the characters
  that are not letters, digits or spaces are dropped, as in a GitHub anchor).

A section stands for the ones below it: a case of `formats.md#7.2` is a case of
`formats.md#7`. `run.py --section` takes a reference, or a glob on it, and `--sections`
lists what each section has. Under a section a case is only a check of its contract rules,
not of all its statements; the coverage table says which contracts are checked.

## Contract and Free

The specification marks each rule **Contract** (the same in every implementation) or
**Free** (messages, prompts, caches). The suite follows it like this:

- **Always Contract**: the exit code of every step, and every file written (`work/`,
  `home/`): the bibtex, `.papers/`, the global configuration, the history store, the
  files moved or copied. A file of the history that has a Free key is compared without it:
  `message` of a state (`data/history/projects/*/states/*.json`, `history.md#3.2`: "free;
  for display"); a difference in that key alone is a Free difference.
- **Contract by default, for the standard output**, of a step that
  - has `--porcelain` (`formats.md#7`: the standard output has the porcelain lines only);
  - is a `list` with `--format key|bibtex|ris|csl-json|bibjson`, `--plain`, `-K`/`--key-only`
    or `--field` (`formats.md#6`).
- **Free by default**: every other output, standard output and error. The messages and
  listings of the commands (`Added -> ...`, `papers status`, `papers config`, `papers
  history`, `papers tag`, the default `list` format, which `formats.md#6` calls "human
  output; should match"), the warnings and errors (`papers: error: ...`), the usage
  messages of exit status 2, and the standard error of a `--porcelain` step.
- **Overrides**: a step can list its Contract streams, `"contract": ["stdout", "stderr"]`
  (`[]` for none). `porcelain/streams` has `list --format bibtex --porcelain`, which sends
  the listing to the standard error (`formats.md#7.1`), as Contract on both.

The rules are `default_contract()` and `FREE_JSON_KEYS` in `conformance.py`.

## The sandbox

Each case runs in a fresh temporary folder with two parts: `work/`, a copy of
`before/`, where the steps run, and `home/`, the HOME of the user. Nothing
else of the machine is seen: the global configuration of papers and its
history are those of this `home/` (platform directories: on macOS
`~/Library/Application Support/papers` and `~/Library/Caches/papers`, on Linux
`~/.config/papers`, `~/.local/share/papers` and `~/.cache/papers`; the XDG
variables are unset). The environment of a step is `PATH` and:

```
HOME=<sandbox>/home  LANG=C.UTF-8  LC_ALL=C.UTF-8  TZ=UTC  COLUMNS=80  LINES=24  NO_COLOR=1  TERM=dumb
```

`COLUMNS` fixes how argparse wraps the usage messages of errors.

## Placeholders and normalization

What is machine-specific is replaced by a placeholder in everything that is
recorded and compared (standard output and error, text files of the trees);
the same placeholders can be written in `before/`, `before-home/`, `argv` and
`stdin` and are replaced by the real paths when the case runs.

| placeholder | what |
|---|---|
| `$WORK` | the working folder (`<sandbox>/work`), as an absolute path |
| `$HOME` | the HOME of the sandbox |
| `$CONFIG` | the folder of the global configuration of papers (`config.json` is in it) |
| `$DATA/history` | the history store of the machine (`$DATA`: the data folder of papers) |
| `$CACHE` | the cache folder (`~/Library/Caches/papers`, `~/.cache/papers`) |
| `$TMP` | the sandbox itself (a path that leaves `$WORK`, e.g. `../x`) |
| `$VERSION` | the version of the implementation: what `--version` prints |
| `$TIME` | a time: `(2026-10-22 14:03)` in `papers history` and `undo`/`redo` lines becomes `($TIME)`, and the `time` of the history files |
| `$PROJECT1`, `$PROJECT2`... | the identifiers of libraries (see below) |
| `$ID` | the `id` of a library configuration that no step of the history names (`papers export` makes one): 32 hexadecimal characters |
| `$MTIME` | in an origin record (`.papers/origin/*.bib`), the number of `@comment{papers-pulled ...}`; `papers-time = {...}` becomes `{$TIME}` |

The paths are replaced in both forms papers prints them: absolute, and with
`~` for the HOME (`~/Library/Application Support/papers/history`). The
unresolved name of a temporary folder (`/var/...` for `/private/var/...` on
macOS) is replaced too.

**Library identifiers.** The first command that records a step of the history
gives the library an identifier, a random uuid, written as `"id"` in
`.papers/config.json`; it names the project of the library in the history
store and is shown by `papers history --all`. The identifiers are replaced by
`$PROJECT1`, `$PROJECT2`... in the order of the places of the libraries (the
paths of their bibtex files). An implementation can use any identifier.

## What is compared

- **Per step:** the standard output, the standard error and the exit code,
  byte for byte (after the placeholders).
- **The working folder after:** every file, byte for byte, and the empty
  folders. This includes the files the case started with (nothing may change
  that should not), and `.papers/` (`config.json`, `.gitignore`...).
- **The HOME after**, as three roots: `config/` (the global configuration:
  `config.json`), `data/` (the history store: `data/history/projects/$PROJECTn/heads.json`,
  `.../states/NNNNNN.json`, `data/history/objects/...`), and `home/` (anything
  else written under HOME). On macOS the configuration and the data folder
  are the same folder, and what is under `history/` is `data/`, the rest
  `config/`.
- **The history objects** (`objects/ab/ab12...`, the pieces of the bibtex named
  by their sha256) are zlib-compressed on disk: they are compared
  decompressed, because the bytes of a compressed stream depend on the zlib
  that made it. An implementation must write objects that papers-cli reads.
  An object whose text has a placeholder in it (a path, the times of an origin
  record) is named by the sha256 of its normalized text, and the states, and
  the `sha` of their bibtex, follow: the real names depend on the sandbox.

`--contract-only` leaves out of this what the specification makes Free (see
"Contract and Free"): the outputs of the steps that are not Contract, and the
`message` of the states.

Not compared:

- The parse cache (`$CACHE`, `Library/Caches/papers`): private to each
  implementation.
- File times and permissions.
- `.DS_Store`.

Not (yet) part of the format: symbolic links, binary files other than
history file objects (PDFs: later phases), the network.

## Things in the reference that are not deterministic, or surprising

- The random identifier of the libraries (above). `papers history --all` lists
  the projects sorted by that random identifier, so its output order is not
  reproducible when the machine has several libraries: the cases use it with
  one library only (`history/undo-redo`).
- The history messages (`papers history`) are the arguments joined with
  spaces, without quoting, with `papers` as the program name whatever was run
  (`papers edit x --set journal=Earth System Dynamics (ESD)`).
- Errors of the command line (an option that does not exist, a missing
  argument, a bad value of `-n`) print the usage message of argparse, exit
  status 2: `errors/bad-option` is the only case that has them, so that they
  can be dealt with apart. An implementation must print `papers`, not its own
  name, as the program in these messages.
- The bibtex parser's own warnings (`bibtexparser.splitter: warning: ...`) go
  to standard error, twice, for a bibtex that does not parse (`list/unreadable-bibtex`).
- `papers add` has no `-` argument for the standard input; the bibtex from
  stdin is `papers add /dev/stdin` (`add/from-stdin`). `papers edit --from -`
  and the `-` of `tag`, `rm` and `edit` (keys, one per line) do read it.
- `papers list` words and `--author` etc. search the bibtex as written: a
  LaTeX accent (`Sm{\"i}th`) is not found by `smith`, nor `Müller` by
  `Muller` (`list/search-accents`).
- The one-line listing keeps a trailing space for an entry with neither DOI,
  file nor tag (`plain2019: An entry without file `).
- A command that changes nothing (`edit --unset nosuchfield`, `tag --rm
  notthere`) records no step in the history.
- `--format ris` needs the optional `rispy` package: no case uses it.
- `papers tag --rename` with `--porcelain` prints no line, although `formats.md#7.2` lists
  `modified` for `papers tag` (`tag/rename`).
- `papers file rename` that is refused (a template that leaves the files directory,
  `../{author}`, or an unknown placeholder) prints `papers: error: ...` per entry and exits
  with status 0 (`file/name-template`). `papers edit KEY --key ''` is no rename: the empty
  value is false and the editor opens (a hang without a terminal), so no case has it.
- A CSL-JSON author given as a plain string (`"author": "Doe, Jane"`) is split into one
  name per character (`{D} and {o} and ...`), although `formats.md#2.3` says a plain string
  is a literal name: no case has it.
- `papers add` of a record without key, author, year or DOI writes the key empty
  (`@misc{,`): no case has it.
- A second save of a bibtex that has two failed blocks in a row writes one more warning line
  (`bibtex.md`, open question 1): `bibtex/save-format` saves once.
- `papers tag` lists the tags sorted without case, and the order of two tags that differ only
  by case (`Z`, `z`) is not reproducible: the cases never list both.
- `papers config` of a global configuration of papers 2 (`papersconfig.json`) depends on the
  platform for its location: no case has it.
- A pull right after another change in the same second can miss a change; the cases of
  `exchange` use `wait` (above).
- A step that changes the case of a key only (`edit Doe --key doe`) is a removal and an
  addition, not a rename, and `crossref` does not follow: no case has it (`bibtex.md#7.3`
  says nothing about it).

## Adding a case

Make the folder with `case.json` and `before/` (a library fixture is easiest
made by running `papers init` and `papers add` in a scratch folder with a
temporary `HOME`, and copying the library; remove the `id` from its
`.papers/config.json`), give it its `sections` and, where a step's output is
Contract although the default rules say Free (or the other way round), its `contract`,
then `python3 spec/record.py --papers papers group/name`.
Read the recorded `expected/` before committing it: it is what papers does,
including its mistakes. The recording refuses what is not reproducible (it runs twice).

**Never run the reference on the real machine to record**: `record.py` and
`run.py` always run in a sandbox with a temporary `HOME`; do the same when
running papers by hand to explore (`HOME=$(mktemp -d)`, in a temporary
working folder), so the global configuration and the history of the user are
not touched.

## Extensions of papers-rs (not in the standard)

What papers-rs has that papers does not. An extension is an option: where it is not used,
papers-rs does what papers does (the cases above pass without it), and the cases of this suite
do not cover it. The usage and the help of the commands that have such an option show it,
marked "(papers-rs)" (they are Free): in the full comparison, the usage lines of these commands
differ (`extract/directory`, `extract/errors`, `fetch/usage`); `--contract-only` passes.

- `--openalex` of `add`, `extract` and `fetch`: a full-text search on OpenAlex
  (<https://api.openalex.org>, with an open API) in place of crossref's, where papers has
  `--scholar` (Google Scholar). The e-mail address of `PAPERS_MAILTO` and the key of
  `OPENALEX_API_KEY` are sent when set. See `crates/papers-rs/src/fetch/openalex.rs`.

## Coverage of the specification

The rows are those of the compatibility table of `design/README.md` (what two
implementations must share), with the cases that check them. "none yet" says why.
RIS needs the `rispy` package, which the reference here does not have: no case.

| Contract | Cases | Not covered |
|---|---|---|
| Library layout (`library.md#layout`) | `init/default`, `init/new-directory`, `init/existing-bibtex-and-files-folder`, `init/bibtex-filesdir-paths`, `export/library`, `status/library` | `.papers/` of a bibtex that is not in the library directory (open question b1): none yet |
| `.papers/.gitignore` | `init/default`, `config/gitignore` | |
| `.papers/config.json`: format, keys, which keys are written, reading, `id`, relative paths | `init/*`, `config/set-and-unset`, `config/prune`, `config/set-invalid`, `config/read-library-config`, `config/pool-path-forms`, `config/legacy-papersconfig`, `export/library` | the format of an `id` created by a command is only checked as 32 hexadecimal characters (`$ID`, `$PROJECTn`) |
| Legacy `.papersconfig.json` | `config/legacy-papersconfig` | `papersconfig.json` of papers 2 (global, `migrate` of it): none yet, its location differs by platform |
| Machine configuration: location, keys | `config/global`, `config/global-default`, `config/global-invalid`, `config/global-too-new`, `init/global-default-library`, `init/shallow-global` | the Linux locations (the suite runs on one platform at a time) |
| Settings: names, types, defaults, precedence | `config/get`, `config/show`, `config/set-and-unset`, `config/set-invalid`, `config/precedence`, `init/format-options` | the display of `papers config` is Free: only the files are Contract |
| Finding the library (`-C`, upwards, `--bibtex`) | `config/find-library`, `config/other-directory`, `status/subdirectory`, `status/bibtex-option`, `list/bibtex-option`, `errors/bad-directory`, `errors/no-library`, `config/global-default` | `--bibtex FILE` of a bibtex that has no library and its history (`history.md#5.1`): none yet |
| `pool` setting, self-contained and shallow | `init/pool-relative`, `init/pool-missing`, `init/shallow-global`, `config/pool-path-forms`, `status/shallow-library`, `porcelain/config-pool` | |
| File names of the files directory, name and key templates, slugify, path safety | `file/name-template`, `add/file-links`, `add/update-key`, `add/key-suffix`, `add/csl-json` (keys from the template) | `{journal}` of an entry without journal (open question d1), `--name-*` with Python format specifications (not part of the contract) |
| Lock `.papers/lock` | `lock/stale` | refresh every 2 s, takeover during a command, the pid check of a lock on this host (it depends on the host name): none yet |
| Bibtex reading and output format | `bibtex/save-format` (`tests/sortformat/varied.bib` of papers), `add/bibtex-file`, `add/odd-bibtex-syntax`, `list/unreadable-bibtex`, `list/bibtex-option`, `add/dry-run` | encodings other than UTF-8, a byte order mark, CRLF input: none yet |
| `file` field syntax | `bibtex/file-field-read`, `bibtex/file-field-write`, `add/file-links`, `list/file-filters` | `paths = absolute` rewriting at load (`config/set-and-unset` only sets it) |
| `keywords` field (tags) | `tag/syntax`, `tag/add-remove`, `tag/rename`, `tag/rename-nested`, `tag/list`, `list/tags`, `list/tags-nested` | |
| `ids` field, `crossref` on renames | `edit/key`, `edit/key-validity-crossref`, `errors/unknown-key`, `rm/by-identifier`, `porcelain/check` | `--keep-old-key` repeating a former key (open question d8) |
| Key suffix rule | `add/key-suffix`, `add/update-key` | `check --auto-key` (open question a1) |
| Same-paper rule and normalization | `list/duplicates-rules`, `list/duplicates`, `add/on-duplicate`, `add/no-check-duplicate` | the dialogue (`ask`): needs a terminal |
| `.papers/distinct.json` | `list/distinct-json` (reading, matching, the file not rewritten) | writing it: only the dialogue adds pairs (needs a terminal) |
| Origin records, pool ids | `exchange/pull-merge`, `exchange/push`, `exchange/auto-push`, `exchange/one-time` (the id of a library given by `--pool PATH`), `exchange/legacy-origin` (the legacy `.papers/origin.bib`), `exchange/rename-key`, `exchange/pull-check`, `porcelain/config-pool` | pools with another id form (a pool setting rewritten, a record with `~`): the origin file names are in `tests/expected/pools.json` of papers-rs |
| Matching entries (`exchange.md#4`) | `exchange/pull-merge`, `exchange/push`, `exchange/pull-check` (removed from the pool), `exchange/one-time`, `exchange/add-from-pool` | the finer rules (a pool key reused, the same paper twice, `distinct.json` of both): `tests/expected/pools.json` of papers-rs |
| Three-way merge and its outcomes | `exchange/pull-merge`, `exchange/push`, `exchange/sync` (`keep`, `update`, `skip`, `error`, the pull that leaves conflicts), `exchange/one-time`; two-way merge: `add/on-duplicate`, `porcelain/check` | a link removed in the pool (`exchange.md#15.2`) |
| `pull`, `push`, `sync`, automatic pushes, `add` from a pool | `exchange/*`, `history/undo-pool-moved-on` (undo with a pool), `exchange/undo-auto-push` | the dialogue of a conflict (`ask`: needs a terminal) |
| Shallow library: files and links of the pool | `exchange/shallow-files`, `exchange/auto-push-links`, `exchange/config-pool-files`, `file/fix-pool`, `exchange/auto-push` (`rm`) | |
| `exchanges.json` | none: it holds the checksums of the texts of the library, the pool and the origin file, and the origin file has times (the `exchange` cases ignore the history store). `crates/papers-rs-cli/tests/pools_interop.rs` has papers and papers-rs write the same bytes | |
| History store: location, layout | `history/*`, every case that writes a step (`home/data/history`) | `history-dir` (the store is not under the data folder, so it is not decompressed for the comparison): none yet |
| Objects, the cutting rule | `history/workflow`, `history/undo-redo`, `history/restore` (small bibtex: one piece) | the rule for libraries of more than one piece: none yet |
| File objects (deleted files) | `history/undo-deleted-files`, `rm/keep-files` | |
| States: numbers, keys, kinds, kept `.papers/` files, file operations | `history/undo-redo`, `history/workflow`, `history/restore`, `history/undo-deleted-files`, `add/file-links`, `config/legacy-papersconfig` | `outside` states (a change made between two commands: a step cannot edit a file yet) |
| `heads.json` | `history/two-libraries`, `history/undo-redo` | |
| Writing order, retention | none yet: needs `history-size` and a store larger than the limit | |
| Separate stores | none yet (`history-separate`, a store in the library) | |
| Project id of a library and of a bibtex without library | `history/two-libraries`, `history/workflow` | the project of a bibtex without library: none yet |
| `.papers/files.json` | `file/checksum`, `add/file-links` | |
| Parse cache | not compared (private to each implementation) | |
| Export formats: CSL-JSON, BibJSON, JSON text, `papers export` layout | `list/csl-json-types`, `list/bibjson-fields`, `list/json-text`, `list/formats`, `list/empty-library`, `list/no-match-formats`, `export/library`, `export/formats` | RIS (needs `rispy`): none yet; import: `add/csl-json`, `add/bibjson`, `add/json-errors`, `edit/from-json`, `edit/from-file` |
| `papers list --format`, `-K`, `--field` | `list/formats`, `list/line-format`, `list/no-match-formats`, `list/empty-library`, `list/bibjson-fields`, `bibtex/file-field-read` | colours (`--color always`): Free |
| `--porcelain` lines and exit codes | `porcelain/order`, `porcelain/streams`, `porcelain/file-commands`, `porcelain/check`, `porcelain/config-pool`, `add/porcelain`, `add/from-stdin`, `add/no-check-duplicate`, `add/on-duplicate`, `edit/key`, `edit/from-stdin`, `edit/set-several-entries`, `rm/remove`, `tag/add-remove`, `tag/rename`, `errors/*`, `config/read-library-config`, `edit/key-validity-crossref` | `papers add` of a PDF or a DOI, `pull` lines: none (offline, no PDF) |

Not covered by the specification: fetching, `papers extract`, PDF metadata, the
interactive dialogues.
