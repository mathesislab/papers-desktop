# Performance: papers-rs and papers

Measured 2026-10-29 on a MacBook (Apple silicon), papers-cli v3 at 0941475
(3.0.0b7.dev3, Python 3.14) and papers-rs following it (release build), with
`tools/bench.py` on a copy of a real library: 2,346 entries, 1.4 MB of
bibtex. Median of 5 runs, in seconds; each program with its own temporary
HOME. For papers, the runs after the first have its parse cache.

| command | papers | papers-rs | × |
|---|---|---|---|
| `list`, first run (empty HOME) | 0.290 | 0.040 | 7.3 |
| `list` | 0.160 | 0.041 | 3.9 |
| `list --format bibtex` | 0.178 | 0.025 | 7.2 |
| `list --format csl-json` | 0.281 | 0.048 | 5.8 |
| `list WORDS` | 0.145 | 0.028 | 5.2 |
| `list --duplicates` | 0.207 | 0.033 | 6.3 |
| `tag --add` (write) | 0.203 | 0.051 | 4.0 |
| `tag --rm` (write) | 0.202 | 0.050 | 4.0 |
| `add` bibtex (write) | 0.234 | 0.052 | 4.5 |
| `rm` (write) | 0.195 | 0.057 | 3.4 |

- papers-rs is 3 to 7 times faster. Reading gains most; a write gains less,
  as both spend part of it on the lock, the history store and the save.
- At this size both answer in a fraction of a second: the difference
  matters for larger libraries and for the app, which writes through
  papers-rs in its own process instead of starting Python for each change.
- papers' parse cache halves its time from the first run to the next;
  papers-rs has none.
- Compared with the first measure (2026-10-28, papers 3.0.0b5), both are a
  little slower (papers by 0.01–0.05 s, papers-rs by 0.01 s): v3 does more
  at load (the `.papers/` layout, relative links, the configuration checks).

To measure again (the library is copied, and only read):

    python3 tools/bench.py LIBRARY.bib --papers papers --papers-rs ../../target/release/papers-rs
