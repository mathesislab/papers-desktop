Papers Desktop (https://github.com/mathesislab/papers-desktop) will show nested tags: a tag with `/` in it, such as `climate/amoc`, is under `climate`. The tags stay plain strings in `keywords`, so nothing changes in the bibtex, and papers already writes them. Checked against `v3` (6d8b3f3).

Proposal, so that papers and the app agree:

- `list --tag climate` also finds the tags under it (`climate/amoc`, `climate/amoc/tipping`), not `climate-review`. `--tag climate/amoc` is that tag and the ones under it. Today `--tag climate` finds `climate` only, and `--tag 'climate/*'` the ones under it only.
- `tag --rename climate ocean` renames the tags under it too: `climate/amoc` becomes `ocean/amoc`.
- Optional: `papers tag`, listing the tags with their counts, could show them as a tree, a parent counting the entries under it.

A tag without `/` works as now. The separator `/` is that of Obsidian and Bear (JabRef uses `>` for its hierarchical keywords). Small: `_has_tag` in `cli/entry_cmds.py` and the rename.
