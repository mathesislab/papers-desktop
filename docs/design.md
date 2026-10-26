# Design and plans

## Principles

- The library is one or more `.bib` files, read as they are (with the
  `biblatex` crate), and the files their entries link to. The app works out
  of the box, with no other tool: papers, JabRef or any editor keeps the
  `.bib` files. It writes nothing yet.
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

## Next: the main window

Two windows with different jobs:

- **Quick panel**, as now: a transient launcher. It floats and hides when
  it loses the focus, which suits a lookup but not a paper kept open while
  writing.
- **Main window**: a normal window (in the Dock and `⌘Tab`) that stays
  open, with a tab bar drawn by the app (the same on every platform):
  - a fixed **Library** tab first: the search list and preview of the
    panel for now, a table later (sortable columns, multi-select, batch
    actions);
  - one tab per paper. `⌘I` in the panel or in the Library tab opens the
    paper's tab (or goes to it) and brings the window forward. `⌘W` closes
    a tab, `⌃Tab` and `⌘1`–`⌘9` switch. The open tabs are remembered
    between sessions.
  - It opens from the menu-bar icon, with `⌘O` in the panel, and perhaps a
    global hotkey of its own.

A **paper tab** (card):

- left: all the information of the paper — the BibTeX fields of the entry
  (parsed in the core), the
  files (open, reveal) and the tags;
- right: the preview of the panel as it is (first page, citation, BibTeX
  folded).

It is read-only at first.

## Later: editing

To decide before anything writes: through papers when it is installed (its
file naming, undo and git history), and maybe natively for small changes
(an entry rewritten in place, the rest of the file untouched). Through
papers:

- **Tags**: `papers tag KEY --add TAG` / `--rm TAG`, applied at once.
- **Fields**: `papers edit KEY` takes the edited entry, either through the
  `editor` setting of papers pointed at a command that writes the edited
  entry (works today), or through a non-interactive option to add to papers
  (e.g. `papers edit KEY --from FILE`).
- Fields are saved with `⌘S`, not on every change: each papers write is one
  undo step and one git commit, and autosaving would write half-typed
  values. A tab with unsaved edits shows a dot, and closing it asks first.

**Groups are tags** (the BibTeX `keywords` field, which JabRef reads too):
e.g. `amoc-paper` for the references of a manuscript. `⌘T` in the panel
tags the selected paper; the library view exports the papers of a tag to a
project's `references.bib` (for texmark).

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
