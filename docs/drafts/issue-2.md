Papers Desktop (https://github.com/mathesislab/papers-desktop), a search panel and library window over BibTeX files, writes a papers library through the `papers` command, so that a change from the app or from a terminal has the same format, naming and history. This would make papers a safe backend for it, and helps scripts and editors as well. Checked against `v3` (2bb1e05) and tested with 3.0.0b1.

A write prints `Added|Modified|Removed -> key: title` (`_print_biblio_diff` in `cli/common.py`) and errors as `ERROR:papers:...` on stderr. `list --format csl-json` is lossy and slow (9 s for 2342 entries; `bibtex` takes 0.2 s).

Proposal:

- A global `--json`: a write prints one object, `{"added": [...], "modified": [...], "removed": [...]}` with the keys (and notes); an error prints `{"error": "..."}` and exits 1.
- `list --format json`: per entry the key, the type, the raw fields, the tags and the files (absolute paths), without the LaTeX conversion.

Small to medium: the first is a variant of `_print_biblio_diff`.
