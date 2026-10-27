papers edit only works through an editor, and an edit that fails or is rejected still exits 0, so a script or another program cannot use it or tell whether it worked.

- `papers edit KEY --from FILE` (`-` for the standard input) replaces the entries by the entries of FILE, with the checks of an edit in the editor: it parses, no failed blocks, valid field names, an entry left. With several keys, FILE has their entries, as the editor text does. `--from -` with keys from the standard input (`edit -`) is an error.
- An edit that is rejected, from the editor or from `--from`, exits 1 and keeps the entries (also `papers list --edit`).

`edit_entries` keeps its behaviour for `papers add -e`; the checks moved to `parse_edited`, which `edit_selected` calls for `--from`. Four test mocks of `edit_entries` returned the list they were given, which `edit_selected` now reads as rejected: they return a copy.

Tests for `--from` (file, stdin, several keys), rejected edits (exit status, entry kept), and the one commit in the git backup. Documented in docs/listing.md.

Closes #117.
