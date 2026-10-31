# Feeds: design proposals

Branch `feed-reader`. Sources: the app's code (`docs/design.md` "Later: feeds"),
paper-feeder (`../paper-feeder`), Feedly, Inoreader, NetNewsWire, Zotero feeds,
Scholar Inbox, arxiv-sanity-lite, and the Crossref, OpenAlex and arXiv APIs.

## Common to all three

**Place.** A tab bar at the top of the main window: *Library* | *Feeds*
(⌘1, ⌘2). The Feeds tab has the library's layout: a sidebar, the search
field, a table, and a details pane. The details pane shows the abstract
in full, because it is the basis for deciding.

**Sources.**
- Journal RSS/Atom (Copernicus, Nature, AGU/Wiley, Science, Elsevier) and
  arXiv, with `feed-rs`. paper-feeder's fixes are ported:
  - Copernicus authors and abstract come from the description.
  - Wiley packs all authors into one string, which is split.
  - The DOI is taken from the guid, `prism:doi` or the link.
  - Wiley and Science need a browser user agent.
- Crossref by ISSN, for journals without a feed. It is polled
  incrementally with `from-index-date`, in the polite pool (3 lists/s).
- OpenAlex queries: a topic, an author, or the papers that cite one's
  own (`cites:`). An OpenAlex key is now required, and the app's
  settings already have `openalex_key`.
- Requests are conditional (ETag and Last-Modified), one at a time per
  host. Feed URLs are canonicalised, so the same feed cannot be
  subscribed twice.

**One card per paper.** The items of all sources are merged into one
record, matched in this order:
1. The DOI, normalised.
2. The arXiv id, without its version.
3. Crossref's `has-preprint` and `is-preprint-of` links. For example,
   EGUsphere `egusphere-2025-6066` is the preprint of `tc-20-5895-2026`.
   EGUsphere's review comments (`-RC1`, `-AC1`) are left out.
4. The title, fuzzily, among items with the same first author and a year
   within ±1. A match needs a Jaccard of 0.9 or more.

The card lists the sources it was seen in, and links the preprint and the
published version. When only the preprint is known, it carries a
*preprint* badge.

**In library.** Each card is checked against the library through
`papers_core::Identity` (DOI, or title and year), and gets an *In library*
badge. When the library holds a paper's preprint, the published paper
shows *published version of KEY*.

**Abstract.** When the source has no abstract, it is taken from
Crossref or OpenAlex by DOI. A card without any abstract is marked so.

**Add.** `Add.svelte` takes an `initial` DOI (or arXiv id) and runs `go()`.
That reuses the sheet's existing steps:
- the metadata lookup, then the free PDF;
- the duplicate check and the key;
- the merge.

*Open in browser* (`o`) is there too. The sheet's BibTeX path does not
look for the free PDF, so a prefill always goes through the DOI.

**State.** All of it lives in `app_data_dir`, never in the .bib:
- the subscriptions;
- the items;
- each item's triage state: new → seen → shortlisted, dismissed or added.

## Proposal 1: Reader (Feedly, NetNewsWire)

A chronological reader, with as little added as possible.

- Sidebar:
  - **Views**: Unread, Today, Shortlist, Added, All.
  - **Folders** of feeds, each with its unread count.
  - **Saved searches**.
- Filter: the library's query language (`a: t: j: y: doi: has:`, `-`), plus
  a mute list. A saved search is a virtual feed.
- Triage:
  - `j`/`k` move; an item becomes seen when it scrolls past.
  - `s` shortlists, `x` dismisses, `o` opens in the browser, `a` adds.
  - *Add shortlist* opens the Add sheet for each shortlisted paper in
    turn: Add or Skip, then the next.
- Storage: JSON (`feeds.json` and `feed-items.json`). Seen and dismissed
  items are pruned after N days.
- Effort: small. It is mostly the RSS parser, the merging, the tab, and
  the Add prefill.
- Weakness: you have to read everything. With 40 or more journals that
  is hundreds of items a week, and the search narrows them only on demand.

## Proposal 2: Filtered stream (Inoreader rules, paper-feeder scoring)

All feeds come in. Filters and scores distil them, and every filter
explains why an item matched.

- Sidebar (facets with counts, like the library's Views and Tags):
  - **Views**: Unread, Shortlist, Added, In library, Preprints, Cites me.
  - **Interests**: named, weighted keyword profiles, ported from
    paper-feeder's include and exclude terms. `ice sheet` matches
    `ice-sheet`, and a term in the title scores double. Several profiles
    can exist (for example *AMOC*, *ice sheets*, *methods*). Each one is a
    view sorted by score.
  - **Journals** and **Sources**, with counts. A click filters, as tags do.
- Search: the library's query language, extended with `abs:`, `src:`,
  `score>3`, `is:preprint`, `in:library` and `prof:amoc`. Search and facets
  combine as in the library (chips).
- Rules (condition → action):
  - a profile score ≥ N → shortlist;
  - an author followed → shortlist;
  - an editorial (*News & Views*, *Comment*) → dismiss;
  - a term → mute.
- Every card shows a score, the terms that matched (highlighted in the
  title and abstract), and the rule that acted.
- An optional score column, *Like my library*: BM25 against the
  library's titles and abstracts, offline.
- Triage: as in proposal 1, plus *Mark all above as seen*, so a filtered
  view can be swept quickly.
- Storage: SQLite with FTS5. This is a cache, not truth. Thousands of
  items, facet counts and full text are what it does well. A JSON file per
  subscription and profile list stays, so the cache can be rebuilt.
- Effort: medium. The query language in `crates/core` has to work on feed
  items as well as library entries.

## Proposal 3: Triage desk (Scholar Inbox, arxiv-sanity)

Proposal 2's stream and filters, with a separate **Review** mode for
deciding to download.

- **Skim** is a dense list (title, journal, score, badges). `s` shortlists
  and `x` dismisses. Nothing opens.
- **Review** steps through the Shortlist one paper at a time, in a
  full-width card that shows:
  - the full abstract (enriched) and the authors, with followed authors
    marked;
  - the journal and type: preprint or published, and the link between
    them;
  - **Free PDF?** from Unpaywall or OpenAlex, looked up when the card
    opens: a free PDF, the publisher only, or a preprint PDF;
  - **Related in library**: the 3 closest library papers (BM25, later
    embeddings), with links;
  - **Cites you** or *cites N library papers*, from OpenAlex.
- The keys in Review:
  - `a`: Add, which opens the Add sheet prefilled with the free PDF
    already found, then moves to the next card.
  - `b`: Add without PDF.
  - `o`: open in the browser.
  - `x`: dismiss.
  - `l`: later, which keeps the paper on the shortlist.
- Learning: shortlist, add and dismiss are positive and negative
  examples. Proposal 2's score is joined by a ranker trained on them
  (arxiv-sanity's linear SVM over TF-IDF, then optionally local embeddings
  with `fastembed`). It stays explainable: "like X, Y in your library".
- Effort: large. Proposal 2 comes first, then Review mode, then the
  ranker.

## Items that are not papers (news, press releases, blogs)

Mockup: `docs/mockups/feeds.html`, views 1 to 3.

**One tab, kinds as a facet.** Every item has a kind: paper, preprint,
news & press, or blog.
- The subscription gives the default kind: a journal, a preprint
  server, news & press, or a blog. The kind is guessed when subscribing,
  and the user can change it.
- Each item can correct its kind: a blog item that carries the DOI of
  the paper it is about becomes a post linked to that paper.
- A segment *Papers | Posts | All* picks the kinds. *Papers* is the
  default, so the scientific stream stays clean.
- Search can filter on `kind:`, and the interests and rules apply to
  every kind.

A separate tab was rejected, because the link between a post and its
paper is the point. A press release about a paper in the feed marks that
paper *Covered by ScienceDaily, phys.org*, which is a cheap signal of
importance. The post, in turn, shows *Linked paper* with *Add* and
*Shortlist*, without opening the website. That needs both kinds in one
store and one merging step. The tab, the search and the triage are the
same for both. A post's details pane shows the text of the post instead of
an abstract, and *Linked paper* instead of *PDF*.

**Finding the linked paper.** Checked live on 13 pages of 9 sources.
1. The item in the feed. Futurity puts "Original Study DOI" in 10 items
   out of 10. Eos and RealClimate link the paper in the body.
2. The item's page, fetched when the item is selected or when the user
   presses `F`. A source can be marked so this happens when it is
   fetched instead. Labelled blocks are trusted first:
   - Futurity: "Original Study DOI";
   - ScienceDaily: "Journal Reference";
   - phys.org: "More information" and `a[data-doi]`;
   - Eos: the closing "(*Journal*, doi)".

   phys.org and ScienceDaily have teasers only, so their DOI comes from
   the page in every case.
3. Otherwise, the DOIs and publisher links in the first paragraphs. The
   following are left out:
   - reference lists, sidebars, and "Related" links;
   - the page's own `citation_doi` (The Conversation's is its own);
   - the source's own DOI prefix (Eos `10.1029/…EO…`).

   The candidates are ranked by how often they appear, where they appear,
   and their date. When none clearly wins, up to 3 are shown with a
   *low confidence* mark.
4. Otherwise, the title is searched in Crossref, and kept only if the
   match is close.
5. Otherwise, the user opens the post ↗ and adds by URL, as now: the
   Add sheet already reads `citation_doi` from an article page.

Per source: Futurity, ScienceDaily, phys.org and Eos give one paper
reliably. Carbon Brief and The Conversation cite several papers, and are
often about none, so they give candidates at most. EurekAlert has no
working RSS feed (its `/rss/*.xml` URLs return 404); its pages can be read
through Futurity or phys.org reposts.

## Architecture: a crate, a CLI, the app

The same split as papers-rs and core:

- **`crates/feeds`** is UI-free. It holds:
  - subscriptions and OPML;
  - fetching (conditional GET, per-host limits, through papers-rs
    `Transport`, so tests replay recorded answers);
  - parsing (`feed-rs`, plus paper-feeder's fixes);
  - the Crossref, OpenAlex and arXiv sweeps;
  - kinds and linked-paper resolution;
  - merging, the store, and enriching abstracts;
  - interests, rules and scores.

  It knows nothing of the .bib.
- **`crates/feeds-cli`** (`feeds`):
  - `feeds subscribe <url> [--kind]`, `feeds fetch`;
  - `feeds list [query] [--json]`, `feeds show <id>`;
  - `feeds resolve <id>`, `feeds mark <id> seen|short|dismiss`;
  - `feeds export --rss|--opml`.

  It is testable and scriptable without the app. With `export --rss` it
  can stand in for paper-feeder (a cron job writing a digest), as a later
  step.
- **The app** (`src-tauri/src/feeds.rs`) joins `feeds` and `core`. Its
  job is the *In library* checks (`Identity`) and Add. The UI is in
  `src/lib/feeds/`.
- **The query language.** The parser and matcher move into a small crate
  (or a module of core with no .bib types), over a `Fields` trait. Library
  entries and feed items each implement it. This is the refactor of note 2
  below.

## Reviewer notes (risks and decisions)

1. **Privacy principle.** design.md says the app sends nothing unless the
   user acts. Background polling breaks that. Fetching when the Feeds tab
   opens, or on ⌘R, keeps it. Background polling could be opt-in.
2. **Query language.** `query::parse` and `Fields` in `crates/core` are
   tied to `Library` and `BibEntry`. The proper fix is for the parser and
   the matcher to work on a record trait that library items and feed items
   both implement, not a second parser in TS. That refactor is its own
   commit, before the feeds work.
3. **The main page.** `src/routes/library/+page.svelte` is 1504 lines.
   Adding a tab needs the library pulled out into a `LibraryView`
   component first, as a separate refactor commit. The native PDF view
   lies over the page, so it has to be unmounted or covered when Feeds is
   shown.
4. **Publisher feeds not verified:** Wiley/AGU, Elsevier, IOP and IGS
   (Cambridge returned 403). Their abstracts may be teasers only, which is
   why the abstract is enriched by DOI. Copernicus, arXiv, Nature,
   Crossref and Unpaywall were checked live.
5. **OpenAlex** now needs a key and has a daily credit budget. Its sweeps
   should be few and incremental. OpenAlex does not merge a preprint with
   its published version, so the merging is done in the app.
6. **Add for arXiv-only items.** It has to be checked that
   `fetch_bibtex_by_doi` and `classify_input` take an arXiv id.
7. **Volume.** About 50 journals, an OpenAlex topic and arXiv
   physics.ao-ph come to roughly 300–1500 items a week. JSON with pruning
   suffices for proposal 1. Proposals 2 and 3 want SQLite.
