#!/bin/sh
# Post the papers proposals as issues on perrette/papers (one each).
cd "$(dirname "$0")" || exit 1
gh issue create --repo perrette/papers --title "edit KEY --from FILE: a non-interactive edit, and an edit that fails exits 1" --body-file issue-1.md
gh issue create --repo perrette/papers --title "--json: machine-readable results of the writes, and list --format json" --body-file issue-2.md
gh issue create --repo perrette/papers --title "Safe writes: atomic, and refused when the bibtex changed since it was read" --body-file issue-3.md
gh issue create --repo perrette/papers --title "Changes made outside papers, in the history (and papers backup record)" --body-file issue-4.md
gh issue create --repo perrette/papers --title "Smaller points found while testing 3.0.0b1" --body-file issue-5.md
