Papers Desktop (https://github.com/mathesislab/papers-desktop), a search panel and library window over BibTeX files, writes a papers library through the `papers` command, so that a change from the app or from a terminal has the same format, naming and history. This would make papers a safe backend for it, and helps scripts and editors as well. Checked against `v3` (2bb1e05) and tested with 3.0.0b1.

The backup snapshot is made after a command writes (`backup_bib` in `backup.py` copies the bibtex). A change made outside papers (an editor, JabRef, an app) therefore goes into the next papers commit, mixed with that command, and `papers undo` before any other command restores the last snapshot and drops it without a word. Tested: a title edited in an editor, then `papers undo`: the edit is gone.

Proposal:

- Before a command writes, and before undo, redo and restore: when the bibtex differs from the last snapshot, commit it first as a state of its own ("change outside papers").
- `papers backup record [-m MESSAGE]` does that on request, so that another tool can put its change in the history with a message of its own.

Undo then steps back over an outside change like any other, and nothing is lost. Small to medium.
