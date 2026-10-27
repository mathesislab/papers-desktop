# Proposals for papers 3 (drafts)

Drafts for [perrette/papers](https://github.com/perrette/papers), as a
comment on the feedback issue (#115) or one issue each. Checked against
`origin/v3` (2bb1e05, 2026-10-26) and tested with 3.0.0b1.

## Intro

Papers Desktop (https://github.com/mathesislab/papers-desktop) is a search
panel and, soon, a library window over BibTeX files. It reads the library
itself, and would write a papers library through papers: the same file
format, key and file naming, and one history, whether a change comes from
the app or the terminal. Four small additions would make papers a safe
backend for it, and they help scripts and editors as well.

## 1. Non-interactive edit: `papers edit KEY --from FILE`

`papers edit` only works through an editor: `edit_text` (`utils.py`) runs
`os.system(f'{editor} {file}')`, and the `editor` setting comes before
`$VISUAL`/`$EDITOR`, so a caller cannot replace it. An edit that fails or
does not parse logs an error and keeps the entry, but the command exits 0
(`edit_entries` and `edit_selected` in `cli/common.py`), so a caller cannot
tell.

Proposal:

- `papers edit KEY --from FILE` (`-` for stdin): the entry is replaced by
  the entry in FILE, with the checks that `edit_entries` makes now (it
  parses, no failed blocks, valid field names).
- An edit that is rejected exits 1, with `--from` and with the editor.

About 20 lines.

## 2. Machine-readable results: `--json`

A write prints `Added|Modified|Removed -> key: title` (`_print_biblio_diff`
in `cli/common.py`) and errors as `ERROR:papers:...` on stderr. `list
--format csl-json` is lossy and slow (9 s for 2342 entries; `bibtex`
takes 0.2 s).

Proposal:

- A global `--json`: a write prints one object,
  `{"added": [...], "modified": [...], "removed": [...]}` with the keys
  (and notes); an error prints `{"error": "..."}` and exits 1.
- `list --format json`: per entry the key, the type, the raw fields, the
  tags and the files (absolute paths), without the LaTeX conversion.

Small to medium: the first is a variant of `_print_biblio_diff`.

## 3. Safe writes

`Biblio.save` (`bib.py`) copies the file to its backup, then writes it
with `open(bibtex, 'w')`. The write is not atomic: a crash, or a sync
client (Google Drive, Dropbox) that reads the file meanwhile, sees a
partial file. Two programs that write the same library (papers in a
terminal, an app, JabRef) can overwrite each other's change.

Proposal:

- Write to a temporary file next to the bibtex, then `os.replace` it.
- `get_biblio` keeps the modification time and size (or a hash) of the
  bibtex as read. `save` refuses to write when the file changed since then
  ("the bibtex changed since it was read: run the command again") and exits
  1.

Small.

## 4. Changes made outside papers, in the history

The backup snapshot is made after a command writes (`backup_bib` in
`backup.py` copies the bibtex). A change made outside papers (an editor,
JabRef, an app) therefore goes into the next papers commit, mixed with that
command, and `papers undo` before any other command restores the last
snapshot and drops it without a word. Tested: a title edited in an editor,
then `papers undo`: the edit is gone.

Proposal:

- Before a command writes, and before undo, redo and restore: when the
  bibtex differs from the last snapshot, commit it first as a state of its
  own ("change outside papers").
- `papers backup record [-m MESSAGE]` does that on request, so that another
  tool can put its change in the history with a message of its own.

Undo then steps back over an outside change like any other, and nothing is
lost. Small to medium.

## Smaller points found while testing

- `list --tag ice` also finds `ice-review`: the option matches part of the
  whole `keywords` field (`_longmatch` in `cli/entry_cmds.py`), not a tag.
- Field names in capitals (`Title`, `Keywords`, as older JabRef files have
  them) are not seen: `list` shows no title, and `tag --add` makes a second
  `keywords` field.
- `papers tag --add x KEY` takes KEY as a second tag (`--add` takes all the
  words after it); `papers tag KEY --add x` works.
- `papers list | head` ends with a BrokenPipe traceback.
- An entry that does not parse gets one more `% WARNING Parsing failed`
  line at every save.

## For later: papers inside an app bundle

PyMuPDF is AGPL (or a paid licence): an app that ships papers inside its
bundle would have to ship it under the AGPL too. If the PDF reading were an
optional extra (with `pypdf` or `pdfminer.six`, both permissive, as the
default), and `scholarly` too, papers could be bundled with an app, and
installs faster for everyone.
