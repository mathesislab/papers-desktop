# Performance: papers-rs and papers

Measured 2026-10-28 on a MacBook (Apple silicon), papers-cli 3.0.0b5 (Python
3.14) and papers-rs (release build) of the same day, with
`tools/bench.py` on a copy of a real library: 2,344 entries, 1.4 MB of
bibtex. Median of 5 runs, in seconds; each program with its own temporary
HOME. For papers, the runs after the first have its parse cache.

| command | papers | papers-rs | × |
|---|---|---|---|
| `list`, first run (empty HOME) | 0.284 | 0.037 | 7.6 |
| `list` | 0.154 | 0.030 | 5.2 |
| `list --format bibtex` | 0.140 | 0.017 | 8.4 |
| `list --format csl-json` | 0.244 | 0.036 | 6.8 |
| `list WORDS` | 0.126 | 0.019 | 6.5 |
| `list --duplicates` | 0.156 | 0.022 | 7.0 |
| `tag --add` (write) | 0.160 | 0.043 | 3.7 |
| `tag --rm` (write) | 0.161 | 0.042 | 3.8 |
| `add` bibtex (write) | 0.187 | 0.046 | 4.0 |
| `rm` (write) | 0.165 | 0.051 | 3.2 |

- papers-rs is 3 to 8 times faster. Reading gains most; a write gains less,
  as both spend part of it on the lock, the history store and the save.
- At this size both answer in a fraction of a second: the difference
  matters for larger libraries and for the app, which writes through
  papers-rs in its own process instead of starting Python for each change.
- papers' parse cache halves its time from the first run to the next;
  papers-rs has none.

To measure again (the library is copied, and only read):

    python3 tools/bench.py LIBRARY.bib --papers papers --papers-rs ../../target/release/papers-rs
