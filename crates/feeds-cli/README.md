# feeds

The command line of [`papers-feeds`](../feeds): follow journals, preprint servers, news sites and
blogs, fetch what is new, score it by your interests, triage it, and publish a digest. It does what
[paper-feeder](https://github.com/alex-robinson/paper-feeder) did, on the same folder as the
Feeds tab of papers-desktop.

```sh
cargo install --path crates/feeds-cli        # the program is `feeds`

feeds subscribe https://www.nature.com/ngeo.rss
feeds subscribe --issn 1994-0424 --journal "The Cryosphere"      # a Crossref sweep
feeds subscribe --openalex primary_topic.id:T10644               # needs OPENALEX_KEY
feeds fetch --mailto me@example.org
feeds list "sea ice" --sort score --limit 20
feeds show 78dc4b209cfa
feeds mark shortlisted 78dc4b209cfa
feeds digest --rss feed.xml --html index.html --title "New papers" --link https://me.github.io/feed/
```

| Command | |
| --- | --- |
| `subscribe <url> \| --issn <issn> \| --openalex <filter>` | `--kind journal\|preprints\|news\|blog` (arXiv and preprint servers: preprints, else journal), `--title`, `--journal`, `--fetch-pages`, `--boost N` (added to the score of its items, e.g. 2 for "cites my work") |
| `edit <id> --boost N` | change the boost of a subscription (0 for none); the strongest of the subscriptions that list an item counts |
| `unsubscribe <id>`, `subscriptions [--json]` | |
| `fetch` | `--mailto <address>` (env `FEEDS_MAILTO`), `--openalex-key <key>` (env `OPENALEX_KEY`); one request at a time per host; a feed that fails is reported, the others go on |
| `list [QUERY]...` | `--sort score\|newest`, `--kind paper,preprint,news,blog`, `--limit N`, `--dismissed`, `--json`; the query is the app's (`kind:`, `is:`, `src:`, `prof:`, `score:3-`, `abs:`, ...) |
| `show <id> [--json]`, `find <id>` | everything known of an item; the papers behind a post (needs the network) |
| `mark <state> <id>...` | `new`, `seen`, `shortlisted`, `dismissed`, `added` |
| `prune` | the old items no feed lists go (done on opening too) |
| `interests`, `rules`, `config` | what is set; `--json` for the first two |
| `import-paper-feeder <sources.yaml> <scoring.yaml>` | `--interest <name>` (default `Interests`), `--issn-via crossref\|openalex` |
| `digest --rss <file> --html <file>` | `--query` (default `score:3-`), `--days` (7), `--title`, `--link` |

The folder is `--dir`, else `$FEEDS_DIR`, else `papers-feeds/` in the data folder of the platform.
**Do not point it at the folder of the app while the app runs**: nothing locks the files, and one
writes over the other.

## From paper-feeder

`feeds import-paper-feeder config/sources.yaml config/scoring.yaml` makes subscriptions of the
`rss:` entries (the `journal:` is kept; arXiv is preprints), of the `openalex:` queries and the
`crossref:` ISSNs (the ISSNs under `openalex:` are Crossref sweeps, which need no key, or with
`--issn-via openalex` the filter `primary_location.source.issn:<issn>`); sets the lookback; and
turns the lexicon into one interest (`publish_min_score` is its minimum, `title_boost` the
boost). Importing again changes nothing but the interest. Warnings on the error stream list what
has no equivalent yet and is skipped: `always_include` and `score_boost` of a source, the
`lookback_days` of a query, a pattern that is no Rust regex (look-around), and the settings not
used (`window_days`, `retention_days`, `render`, `serendipity_count`, `llm_rerank`). The `mailto`
is not stored: set `FEEDS_MAILTO`.

`feeds digest` is `render.py`: the RSS has the DOI (else the link) as guid, the authors as
`dc:creator`, the journal as category, and "journal — matched: terms" before the abstract; the
page is the same design. It writes the items of the last `--days` days that pass the query, best
score first, not only the fresh ones: readers keep a guid once.

## Library

`feeds_cli::run(args, &Host, out, err)` is the whole program, on a `Host` (the environment, the
clock, the requests' way out): the tests run it with `papers_feeds::http::Recorded`.
