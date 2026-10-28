# The papers standard: conformance suite

What an implementation of papers does, as cases: a library before, a command,
and what it prints and leaves after. papers-cli (Python) is the reference: the
cases are recorded by running it. papers-rs is checked against them.

Shared by the implementations, so part of the standard: the bibtex written,
`.papers/config.json` and the global configuration, the history and the pool's
steps, the lock, the `--porcelain` lines, the keys and file names. Private to
each: caches (the parse cache).
