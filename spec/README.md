# The papers standard: conformance suite

What an implementation of papers does, as cases: a library before, a command,
and what it prints and leaves after. papers-cli (Python) is the reference: the
cases are recorded by running it. papers-rs is checked against them.

Shared by the implementations, so part of the standard: the bibtex written,
`.papers/config.json` and the global configuration, the history and the pool's
steps, the lock, the `--porcelain` lines, the keys and file names. Private to
each: caches (the parse cache).

The cases are plain files, and the two tools are Python 3 scripts with the
standard library only (`conformance.py` is shared by both):

```
python3 spec/run.py --papers papers                      # an implementation on all the cases
python3 spec/run.py --papers target/debug/papers-rs 'list/*' # on the cases that match
python3 spec/record.py --papers papers [CASE...]         # (re)record the expected results
```

`run.py` prints `PASS`/`FAIL` per case, and for a failure the unified diff of
the first difference (`-a`: all of them), then a summary; the exit status is 0
when all pass. `record.py` runs each case twice and writes the results only if
the two runs agree. `CASE` is a glob on the name of the case (`group/name`).
`--list` lists the cases, `--keep` keeps the sandboxes of the run.

## A case

`spec/cases/<group>/<name>/` (the groups are the commands: `init`, `config`,
`status`, `list`, `add`, `edit`, `rm`, `tag`, `history` (undo, redo, restore),
`errors`):

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
  "requires": ["network"],
  "steps": [
    {"argv": ["init", "lib"]},
    {"argv": ["list", "-K"], "cwd": "lib"},
    {"argv": ["edit", "perrette2013", "--from", "-"], "stdin_file": "stdin/03.txt"},
    {"argv": ["tag", "-", "--add", "x"], "stdin": "doe2019\n"}
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

Folders that are empty are kept as an empty file `.emptydir` (git does not
track folders), and `.gitignore` is stored as `.gitignore.spec` (the
`.papers/.gitignore` of a library contains `*`, which would hide its
neighbours). Both are only about storing: the tools restore the real names.

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

## Adding a case

Make the folder with `case.json` and `before/` (a library fixture is easiest
made by running `papers init` and `papers add` in a scratch folder with a
temporary `HOME`, and copying the library; remove the `id` from its
`.papers/config.json`), then `python3 spec/record.py --papers papers group/name`.
Read the recorded `expected/` before committing it: it is what papers does,
including its mistakes.

**Never run the reference on the real machine to record**: `record.py` and
`run.py` always run in a sandbox with a temporary `HOME`; do the same when
running papers by hand to explore (`HOME=$(mktemp -d)`, in a temporary
working folder), so the global configuration and the history of the user are
not touched.
