Papers Desktop (https://github.com/mathesislab/papers-desktop), a search panel and library window over BibTeX files, writes a papers library through the `papers` command, so that a change from the app or from a terminal has the same format, naming and history. This would make papers a safe backend for it, and helps scripts and editors as well. Checked against `v3` (2bb1e05) and tested with 3.0.0b1.

`Biblio.save` (`bib.py`) copies the file to its backup, then writes it with `open(bibtex, 'w')`. The write is not atomic: a crash, or a sync client (Google Drive, Dropbox) that reads the file meanwhile, sees a partial file. Two programs that write the same library (papers in a terminal, an app, JabRef) can overwrite each other's change.

Proposal:

- Write to a temporary file next to the bibtex, then `os.replace` it.
- `get_biblio` keeps the modification time and size (or a hash) of the bibtex as read. `save` refuses to write when the file changed since then ("the bibtex changed since it was read: run the command again") and exits 1.

Small.
