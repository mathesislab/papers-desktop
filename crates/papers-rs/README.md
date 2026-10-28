# papers-rs

A Rust port of [papers-cli](https://github.com/perrette/papers). papers-cli is
the reference: papers-rs does what it does, with the same output and the same
files, so that both can work on the same library. The conformance suite
(`spec/`) checks it.

- `papers-rs`: the library.
- `papers-rs-cli`: the command line, `papers-rs`.

## Modules

- `config`: the library configuration (`.papers/config.json`), the global
  configuration (the default library, the default settings), the settings with
  their sources (library, else global, else default), the library of a
  directory (`-C DIR`, upward search, the default library), and the files
  written as papers writes them (key order, indentation, no trailing newline).
  Paths are strings with Python's `os.path` semantics (`config::paths`).
- `install`: what `papers init` creates.
- `lock`: the lock of a library (`.papers/lock`), a lease with a refresh thread.
- `error`, `log`: papers' errors, and its messages (`papers: warning: ...`).

## The command line

`papers-rs` (the `papers-rs-cli` crate) has the commands, options and errors of
papers. `init` and `config` are ported; the other commands print
`papers: error: not implemented in papers-rs yet` (status 1).

Tests: `cargo test -p papers-rs -p papers-rs-cli`. The cases in
`crates/papers-rs-cli/tests/fixtures/cases` were recorded by running papers-cli
(`tests/record/record_cases.py`), and the help of the commands by
`tests/record/record_help.py` (both run with a `papers` executable; they use
temporary home directories).
