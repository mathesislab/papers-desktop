Papers Desktop (https://github.com/mathesislab/papers-desktop), a search panel and library window over BibTeX files, writes a papers library through the `papers` command, so that a change from the app or from a terminal has the same format, naming and history. This would make papers a safe backend for it, and helps scripts and editors as well. Checked against `v3` (2bb1e05) and tested with 3.0.0b1.

- `list --tag ice` also finds `ice-review`: the option matches part of the whole `keywords` field (`_longmatch` in `cli/entry_cmds.py`), not a tag.
- Field names in capitals (`Title`, `Keywords`, as older JabRef files have them) are not seen: `list` shows no title, and `tag --add` makes a second `keywords` field.
- `papers tag --add x KEY` takes KEY as a second tag (`--add` takes all the words after it); `papers tag KEY --add x` works.
- `papers list | head` ends with a BrokenPipe traceback.
- An entry that does not parse gets one more `% WARNING Parsing failed` line at every save.
