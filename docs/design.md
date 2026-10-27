# Design and plans

## Principles

- The library is one or more `.bib` files, read as they are (with the
  `biblatex` crate), and the files their entries link to. The app works out
  of the box, with no other tool: papers, JabRef or any editor keeps the
  `.bib` files. It writes nothing yet (see *Writing*).
- Everything is in the `.bib` and the PDFs: no account, no database of the
  app as the truth, no settings of the app written in the `.bib`. Tags are
  the `keywords` field, the one grouping there is.
- The PDFs stay plain files in the files directory: any viewer opens them.
- `crates/core` (Rust, no user interface) loads, searches and watches the
  library, and works on any platform: other front ends (a Spotlight add-on,
  a command line) can use it. `src-tauri` is the desktop shell, `src` the
  interface (SvelteKit).

## v0.1.0: the quick panel

A menu-bar app with a global hotkey (`⌥Space`) and a floating panel: search
(words from their start, field filters, phrases, `-` to exclude), results
newest first with the matched words in bold, a year column and a strip of
the matches per year, a preview (first page, citation in a CSL style,
BibTeX), and actions (open, reveal, DOI page, copy key, `[@key]`,
`\cite{key}`, BibTeX). The library reloads when the bibtex file changes.

## Next: the main window (P0, read-only)

Two windows with different jobs:

- **Quick panel**, as now: a transient launcher. It floats and hides when
  it loses the focus, which suits a lookup but not a paper kept open while
  writing.
- **Main window** (mockup: `docs/mockups/main-window.html`): a normal
  window, in the Dock and `⌘Tab` while it is open. It opens from the
  menu-bar icon (Open Library) and with `⌘O` in the panel; `⌘I` in the
  panel opens the selected paper in it.

Three panes, each resized by dragging, the right one hidden on demand:

- **Sidebar**: the libraries (the .bib files); views (all papers, recently
  opened, no PDF, broken file links, possible duplicates: the same DOI, or
  the same title and year); the tags with their counts (a click adds
  `tag:x` to the search); the open papers.
- **Library**: a table of the papers that match the search (the panel's
  syntax): sortable columns (authors, year, title, journal, tags, a file
  marker that tells a missing file), multi-select. Only the rows on screen
  are drawn.
- **Details** of the selected paper: title, authors, tags, file (open,
  reveal), fields, citation, BibTeX (folded).

**Open papers are side tabs**, in the sidebar: author, year and title in
full, as a tab bar cannot. Enter or a double-click opens one; `⌘W` closes
it, which goes back to the library (a setting: or to the next open paper).
A paper tab shows its PDF in the app (pdf.js: text selection, find,
outline), with the details beside it. The open papers, their pages, the
pane widths and the details shown or not are kept in the data folder of
the app, as the recent papers are.

A tag or the selection exports to a `.bib` file, e.g. a manuscript's
`references.bib` (for texmark).

"Recently added" needs a date the `.bib` does not have: see issue #1.

## Writing (P1)

- **A papers library** (a `.papers/config.json` next to its `.bib`) is
  written by papers (3.0 or later), as a command, so that a change from
  the app or from a terminal has the same format, naming and history:
  `papers tag KEY --add TAG` / `--rm TAG`, `papers add`, `papers rm`,
  `papers file attach|detach|rename`, `papers undo|redo`, and `papers edit
  KEY --from FILE` once papers has it (proposals to papers:
  `docs/drafts/papers-proposals.md`). Without papers installed, a papers
  library is read-only, and a window tells how to install it (with copyable
  commands): `pip install papers-cli` or `uv tool install papers-cli`.
- **Any other `.bib`** is written by the app: only the edited entry is
  rewritten, the rest of the file stays byte for byte; the file is written
  to a temporary file then moved, and not at all if it changed since it
  was read.
- Fields are saved with `⌘S`, not on every change: each papers write is one
  undo step and one git commit, and autosaving would write half-typed
  values. A tab with unsaved edits shows a dot, and closing it asks first.
- **Tags**: add, remove, rename; `1`–`9` toggle the pinned tags on the
  selection. **A manuscript's tag** from the citations of a `.md`
  (`[@key]`), `.tex` or `.aux`: its cited papers are tagged, and the keys
  not in the library listed. A tag can keep its project `.bib` up to date.
- **Add**: a DOI or identifier, a PDF dropped in, BibTeX pasted; a
  duplicate check first. **Delete**, attach, detach and rename files.

Towards the app's own writer for everything: it follows the conventions of
papers (`.papers/config.json` for the key and file names, tags in
`keywords`, the `file` field, the format of papers for an edited entry) and
records its changes in the history of papers (`papers backup record`, a
proposal); then papers is optional, still the command line and the source
of metadata from a DOI or a PDF until the app has its own.

## Later (P2)

Merge duplicates; checks (missing DOIs, broken links, keys); saved
searches; full-text search of the PDFs; git sync of the `.bib` files (not
the PDFs) with a remote; RIS and CSL-JSON import and export; annotations
kept in the PDF.

## Later: adding papers without a browser extension

1. Paste a DOI or an article URL in the panel, or *Add current page*
   (Safari and Brave tell their current URL through AppleScript; paste on
   Linux and Windows).
2. The metadata first (`papers fetch DOI`, or the `citation_*` meta tags of
   the page), shown with the key, the file name and a duplicate check.
3. The PDF: downloaded by the app when open access, or when the
   institution's network gives access; otherwise *armed*: the app waits for
   the next PDF in `~/Downloads` (downloaded in the browser, with its
   login), checks its DOI, and runs `papers add PDF --doi DOI`.

## Later: feeds

The new papers of the journals followed, as Feedly lists them: most
journals have an RSS feed of their contents (Copernicus, Nature, Wiley and
AGU, Science, Elsevier), arXiv too, and for a journal without one Crossref
lists its new DOIs by ISSN. A Feeds view: title, authors, journal,
abstract; mark as read; *Add* takes the metadata from the DOI.

The PDF, the simplest way first:

1. Open access: Unpaywall (by DOI, free) gives the link of the PDF.
2. The article's page: its `citation_pdf_url` meta tag (most publishers
   have it, for Google Scholar), fetched on the institute's network or VPN,
   kept if it is a PDF.
3. Otherwise the article opens in the browser, and the app waits for the
   next PDF in `~/Downloads` (as above): logins, viewers and checks happen
   in the browser.

A browser tab in the app (a webview that catches downloads) only if 3 is
too clumsy. One paper at a time, when asked: no downloads in the
background, which publishers block.

## Later, optional: Spotlight add-on

A small Swift helper that indexes the items of the core with Core Spotlight,
so that papers show in the Spotlight of macOS.

## Other platforms

Linux (Ubuntu, GNOME; Wayland by default, X11 available):

- The default hotkey is `Ctrl+Alt+Space`: `Alt+Space` opens the window menu
  of GNOME, KDE and Windows.
- Under Wayland an app cannot grab a global hotkey. A second launch of the
  app toggles the panel of the running app (single instance), so a custom
  desktop shortcut running `papers-desktop` stands in for the hotkey.
- The tray icon is white (`tray-light.png`) and needs AppIndicator support:
  Ubuntu has it, other GNOME desktops need an extension. The app works
  without the icon.
- Bundles: `.deb` and AppImage.

Not tested yet: on Wayland, whether the panel takes the focus when shown and
hides when it loses it. Windows is neither built nor tested (it would take
the same hotkey and the white tray icon).

## Known gaps

- The app icon is the default icon of Tauri.
- Tested on macOS only.
- The reload on a changed bibtex file is tested with a temporary file, not
  yet seen on a live library change.
