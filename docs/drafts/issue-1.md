Papers Desktop (https://github.com/mathesislab/papers-desktop), a search panel and library window over BibTeX files, writes a papers library through the `papers` command, so that a change from the app or from a terminal has the same format, naming and history. This would make papers a safe backend for it, and helps scripts and editors as well. Checked against `v3` (2bb1e05) and tested with 3.0.0b1.

`papers edit` only works through an editor: `edit_text` (`utils.py`) runs `os.system(f'{editor} {file}')`, and the `editor` setting comes before `$VISUAL`/`$EDITOR`, so a caller cannot replace it. An edit that fails or does not parse logs an error and keeps the entry, but the command exits 0 (`edit_entries` and `edit_selected` in `cli/common.py`), so a caller cannot tell.

Proposal:

- `papers edit KEY --from FILE` (`-` for stdin): the entry is replaced by the entry in FILE, with the checks that `edit_entries` makes now (it parses, no failed blocks, valid field names).
- An edit that is rejected exits 1, with `--from` and with the editor.

About 20 lines.
