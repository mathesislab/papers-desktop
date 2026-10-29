# Design and plans

## Principles

- The library is one or more `.bib` files, read as they are (with the
  `biblatex` crate), and the files their entries link to. The app works out
  of the box, with no other tool: papers, JabRef or any editor keeps the
  `.bib` files. It writes a papers library through papers, any other `.bib`
  with its own writer (see *Writing*).
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

## The main window (P0, read-only; 0.5)

Two windows with different jobs:

- **Quick panel**, as now: a transient launcher. It floats and hides when
  it loses the focus, which suits a lookup but not a paper kept open while
  writing.
- **Main window** (mockup: `docs/mockups/main-window.html`): a normal
  window, in the Dock and `⌘Tab` while it is open. It opens from the
  menu-bar icon (Open Library) and with `⌘O` in the panel; `⌘I` in the
  panel opens the selected paper in it.

Three panes, each resized by dragging, the right one hidden on demand:

- **Sidebar**: *Libraries* (with several .bib files: *All libraries*, then
  one row per .bib, as the accounts of Mail); *Views* (all papers, recently
  opened, no PDF, broken file links, possible duplicates: the same DOI, or
  the same title and year); the tags with their counts (a click filters by
  the tag, a second click takes the filter off); *Open*: the library tab,
  then the open papers. The counts of the views and the tags are those
  within the library chosen.
- **Library**: a table of the papers of the current set (below): sortable
  columns (authors, year, title, journal, tags, a file marker that tells a
  missing file), multi-select. Only the rows on screen are drawn. A line
  above it shows the filters that are on as chips, each with an ×:
  `papers.bib › No PDF › amoc › "ice"`; with no filter, there is no line.
- **Details** of the selected paper: title, authors, tags, file (open,
  reveal), fields, citation, BibTeX (folded).

**Tabs.** The window works as a browser does: the first tab is always the
library, then one tab per open paper, listed in the sidebar (*Open*): author,
year and title in full, as a tab bar cannot. Enter or a double-click opens a
paper; `⌘W` closes it, which shows the next open paper, the library after
the last (a setting: or the library at once; `after_close`, "next" by default,
so a settings file that does not have it gets that). With the library in front
`⌘W` closes the window. `⌘⇧]` and `⌘⇧[` (Ctrl+Shift elsewhere; *Show Next Tab* and
*Show Previous Tab* of the Window menu) and `⌃Tab` go through the tabs,
Library → paper 1 → … → Library; `⌘1` shows the library.

**The library tab is the current set**, and keeps it while papers are opened
and closed: going back shows the same list, the selection and where it was
scrolled to. Its sidebar row is `Library` with its count, and under it, in
a second line (indented, smaller, muted, ellipsized), the filters it shows
(`papers.bib · No PDF · amoc · "ice"`; no second line with no filter on). The set is the
product of four filters (`src/lib/library/filters.ts`, a pure function of the
rows, tested by `scripts/filters.test.ts`, `npm test`):

- **Library**: all libraries, or one .bib (a click on its row).
- **View**: all papers, recently opened, no PDF, broken file links, possible
  duplicates. *All papers* takes the view filter off, and only that.
- **Tags**: any number, clicked in the sidebar (or in the details); a paper
  must have all of them (a tag counts the tags under it: `climate` finds
  `climate/amoc`). They are filters of their own, shown as chips and
  highlighted in the sidebar, not text of the search field: the field keeps
  what was typed, and a click on a tag does not change it. A `tag:` typed in
  the field works as before, as part of the search (`tag:`, `-tag:` and the
  rest of the syntax); the core is asked for the search text followed by a
  `tag:` term per tag filter.
- **Search**: the text of the field. It stays when the view, library or tags
  change and when papers are opened, until it is cleared: the × in the field,
  or `⌘⌫` (Ctrl+Backspace elsewhere) in the field, where it clears it;
  elsewhere in the library `⌘⌫` deletes the selection (*Delete*, below), or
  clears the search when nothing is selected. `Esc` leaves the field; `/` and
  `⌘F` go to it.

Library, view, tags and search are kept in the layout (`window.json`, `library`)
and come back at the next start; a .bib file no longer in the library is
dropped. A paper shown after it was added or merged takes the filters off if
they would hide it.

**Several papers at once** (`+page.svelte`, `Table.svelte`, `rowmenu.ts`,
`selection.ts`, pure and tested by `scripts/selection.test.ts`):

- *Open*: Enter, a double-click, or *Open* in the context menu opens every
  paper selected as a tab (`openPapers`, the first in front); a double-click on
  one of several selected opens all of them (a click on one of several selected
  keeps them selected until the button is released, as in Finder, so that they
  can be dragged). Above 5 papers it asks first (*Open 12 papers?*). A paper
  without a file opens its web page in the browser, as before.
- *Drag* rows (the selection if the row is in it, else the row) onto the
  *Open* section of the sidebar: they open as tabs. The drag is made of pointer
  events (`drag.svelte.ts`: the page's own drag and drop is the window's drop of
  PDFs); a label follows the pointer, the section shows a frame and *Drop to
  open*; `Esc` cancels.
- *Open in Preview* (macOS; *Open in default app* elsewhere): the PDFs of the
  selection outside the app (`open_pdfs`, the opener plugin: Preview on macOS,
  the system's default app elsewhere). A paper without a PDF is skipped, and
  the notice says how many (*Opened 3 PDFs, 2 papers without a PDF skipped*).
- *The context menu of a row* (right-click; on the selection if the row is in
  it, else on that row, which becomes the selection): *Open*, *Open in Preview*,
  *Reveal in Finder*, *Open DOI page*; *Copy [@key]* (`[@a; @b]` for several),
  *Copy BibTeX* (one after the other), *Copy citation* (the style of the
  settings, one per line), *Edit BibTeX* (one paper: the editor of the
  details); *Tags ▸* (the tags pinned to the keys, then the most used, each a
  check that puts it on the papers or takes it off; *Edit tags…* opens the field
  of the details); *Add PDF…* (one paper without a file); *Copy to ▸* and *Move
  to ▸* (the other .bib files, when there are several); *Export selection…*;
  *Delete…*. What does not apply is disabled (no PDF, no DOI).
- *Delete…* (the menu, `⌘⌫` on the selection): *Delete 3 papers?* with a block
  per paper (scrolls): its key, its citation in the style of the settings (as
  the preview shows it), and each file it links on a line of its own with a
  checkbox (off; *(no PDF)* when it has none), and a line on where the ticked
  files go. The button says *Delete 3 papers, 2 files*. Only the ticked files
  are deleted. Then the next row is selected.

**Downloads panel** (`Downloads.svelte`, `downloads.rs`, `downloads.ts` with
`scripts/downloads.test.ts`): at the bottom of the sidebar, over the list when
it scrolls, a panel headed *Quick folder view: Downloads* (the folder's name)
for the PDFs that have not been added yet. It is not part of the library, and
looks it: a slightly different background and a dashed top edge. In the header
a button (a folder with an arrow going out) opens the folder in Finder (the
file manager elsewhere), and a menu
(the … button, or a right-click on the panel) has *Choose Folder…*, *Show in
Finder* and *Hide*.

- *The list*: the PDFs of the folder (`.pdf` in any case; not those of the
  folders in it, nor hidden files), newest first by the later of the file's
  modification and creation time, each with its name, size and date (the time
  today, else the day). A long list scrolls within the panel, which takes at
  most a quarter of the window's height. A `notify` watcher on the folder
  (`DirWatch`, events on PDFs only, debounced by half a second, as a download
  is written in pieces and renamed) tells the page (`downloads` event), so
  files appear and go by themselves: a PDF moved into the library leaves the
  folder, and the list.
- *Drag* a PDF onto the library table: the Add sheet with that PDF, as for a PDF
  dropped on the window; onto the empty *No PDF* preview of a paper: the sheet
  to add it to that paper. A double-click opens the sheet with the PDF as a new
  paper. The drag is made of pointer events (`pdfDrag` in `drag.svelte.ts`), as
  the drag of rows is, not the page's drag and drop, which the window's drop of
  PDFs shares the page with; a label follows the pointer, the target shows the
  same frame as a PDF dropped from outside, `Esc` cancels.
- *In the library already* (muted, with a check): a file that a paper links (the
  same path) or that has the same contents as a file of the library (compared
  among files of the same size, so that few are read). So a PDF added with *Copy*
  or *Leave in place* is marked, and is not mistaken for a new one. Which paper
  and which file is told (`Match` of `downloads.rs`: the paper's id and key, its
  file relative to the folder of its .bib, or the full path elsewhere, and
  whether it matched by contents only; a paper once): in the tooltip,
  *in the library: KEY — files/x.pdf* (*same contents as* for a match by
  contents), a line per paper; and the right-click menu of the PDF has *Show KEY
  in the library* (one item each, when several papers have it), which selects
  the paper in the library tab with its details.
- *Settings*: *Downloads panel* (shown by default, also for a `settings.json` from
  before it) and *Downloads folder* (`downloads_panel`, `downloads_folder`:
  `~/Downloads` by default, *Choose…* with the folder dialog). *Hide* in the
  panel's menu and *Choose Folder…* set the same settings (`set_downloads_panel`,
  `set_downloads_folder`), and the settings window follows.
- *Privacy*: only the file names, sizes and dates of that one folder are read
  (and the contents of a PDF of it with the size of a file of the library, to
  compare them); nothing is sent anywhere. Tests (`downloads.rs`) use temporary
  folders, never the real one.

A paper tab shows its PDF in the app, with the details beside it: on macOS
in a native view of PDFKit, as Preview shows it (smooth scrolling and zoom;
later, annotations written in the PDF), placed over the reader area of the
page; elsewhere with pdf.js (to tune when Linux comes: its drawing on the
GPU, fewer redraws). Find, zoom and the outline (a strip at the left of the
PDF) drive either. A native view lies above all of the web view, the modals of
the page (the Add sheet, the delete question, the papers-missing notice) included:
each holds `holdModal` (`modal.svelte.ts`) while it is open, and the page has
the reader step aside (`Reader.cover`: PDFKit's view is hidden, `pdf_cover`,
and the keys go to the page; shown again, with the keys, when the last modal
has closed). A pdf.js PDF is in the page, under the modals by their z-index. The library tab, the open papers, their pages, the pane widths and the
details shown or not are kept in the data folder of the app, as the recent
papers are; the window opens again at start if it was open at the end.

A gear button at the bottom of the sidebar opens the settings window, as the
app menu's Settings… does.

**Text size** (setting, five levels: Smallest 0.9, Smaller 1.0, Default 1.1,
Larger 1.25, Largest 1.4; Smaller is the size before the setting): the zoom of
the web views of all windows (`text_size.rs`, `Webview::set_zoom`, as a
browser's), so everything scales; one level for the windows, kept in
`settings.json` and applied at once. The panel and the settings window, of a
fixed size, grow with it (within the screen). `⌘+` `⌘-` `⌘0` (Ctrl elsewhere)
step through the levels and reset to Default, in every window; in the main
window, with a PDF in front they are the zoom of the PDF (and `⌘0` its fit to
the width) instead. The native PDF view is placed in points, so its frame is
the page's multiplied by the zoom. The View menu (macOS) shows *Zoom In* as
`⌘+`, as Safari does: a menu accelerator of muda is a key, `=`, which would
show `⌘=`, so `menu.rs` sets the key equivalent of the item to `+` once the
menu is made; AppKit does not match `⌘=` to it, so the page takes that key
(`zoomStep`; with the native PDF view in focus `⌘=` does nothing). Elsewhere
there is no menu: the page takes `+` and `=` alike.

A tag or the selection exports to a `.bib` file, e.g. a manuscript's
`references.bib` (for texmark).

"Recently added" needs a date the `.bib` does not have: see issue #1.

## Writing (P1)

- **A papers library** (a `.papers/config.json` next to its `.bib`, naming
  it) is written by papers, so that a change from the app or from a
  terminal has the same format, naming and history. A setting says what
  writes it (*Writes papers libraries*): **papers-rs, built in (the
  default)**: the commands run in the process (`papers_rs::command` and
  `history`), no subprocess, no Python; or **a papers command** (3.0.0b4 or
  later, with `edit --from` and `--porcelain`): any command line of the
  papers standard (papers-cli, the `papers-rs` program, another): the
  `papers` setting, else the one the login shell finds, run with the PATH of
  the login shell (its git) and `--porcelain`, whose output tells the keys
  added and the papers already in the library. They are interchangeable: the
  same files, the same history store and lock, so a step of one is undone by
  the other. In `writer.rs` each is an implementation of `PapersWriter`
  (`writer/builtin.rs`, `writer/command.rs`); the commands of the app group
  the changes by library and hand them to it. Built in, a library with a pool
  is written as the others: the entries it adds go to the pool, and those it
  changes too with `auto-push`, as papers does (a conflict with the pool is
  left to `papers sync`); with a command not found, a papers library is
  read-only, and a dialog tells how to install it, with copyable commands
  (from git, `papers-cli @ git+https://github.com/perrette/papers@v3`,
  until a release has 3.0.0b4).
- **An email** (setting, empty by default, never filled in by the app: not
  the system's, not git's): used only when set. The DOI fetch gives it in
  its User-Agent, `Papers Desktop/<version> (mailto:<email>)`, as Crossref
  and doi.org recommend; OpenAlex takes it as `mailto` (`OpenAlex`): the
  lookup of a free PDF in the sheet that adds a paper (below) sends it. Empty:
  nothing is sent. Next to it, an **OpenAlex API key** (free, empty by
  default, in settings.json like the others, a password field): sent to
  OpenAlex only, as `api_key` of the same `OpenAlex` (`Settings::openalex()`).
- **Add paper** (⌘N, *Add…*): one sheet for a new paper and for a PDF added
  to a paper of the library (mockup of the layout: the PDF, the DOI with where
  it came from, the entry, the file). It replaces `papers add FILE.pdf
  --rename --copy --open`. Everything it reads is read-only and in the
  process, whatever the writer (`src-tauri/src/fetch.rs`, `input.rs`; papers-rs
  with the queries and the cache of papers, the app's user agent):
  - *Opened by*: a PDF dropped on the main window (Tauri drag-drop events,
    anywhere; dropped on the empty page of the paper in the details it is that
    paper's; or dragged from the Downloads panel, below), *Add…* / ⌘N, then *Choose…*, or a text pasted or typed in the
    field, which `classify_input` takes for a DOI or doi.org link, the path of
    a PDF (quoted, with escaped spaces, `file://`, `~`), a link to a PDF
    (downloaded), a link to a page, or BibTeX (the entry).
  - *A PDF*: `read_pdf_doi` reads its DOI as `papers add FILE.pdf` does (the
    DOI of its metadata; else of its text, if the title of that DOI is in the
    text; else a search of Crossref by the text) and fetches its entry; the
    field says where the DOI came from (*from the PDF*, *found by a search of
    the PDF's text*, *typed*, *from the page*, *none found*). *Read PDF again*
    goes back to what the PDF gives; another DOI typed and fetched replaces the
    entry. The BibTeX is a text to fix (folded when the entry has a DOI).
  - *The key shown is the library's*: an entry fetched (Crossref's key is its
    DOI) is shown with the key the library will give it, its key template as
    `--update-key` makes it, a letter after it when the .bib has the key
    (`library_key`, `writer.rs`: the template of the library's configuration
    through the built-in writer's `Config`, papers' default `{author}{year}` for
    a plain .bib; the keys taken are those of the .bib in the app's library).
    That key is in the text, so that what is written is what is shown, and no
    `--update-key` goes with the add; a key the user edits stays. Another
    *To* .bib makes the key again, while it is the one the sheet made.
  - *The same paper as one of the .bib*: when every entry is a paper of the
    .bib to add to already (`Incoming.same`, in that .bib; another .bib has its
    own copies, which are only noted), the sheet becomes *Same paper as KEY*
    (the key is a link: the sheet closes and the paper is selected) and
    compares instead of adding: a table of the fields that differ, *in the
    library | new*, a radio in each cell for the value to keep
    (`compare_entry`, `compare.rs`: papers-rs' two-way `Merge` and `FieldMerge`
    give the rows, so equal values by papers' normal form are not shown, and
    the type is a row, `ENTRYTYPE`). By default the library's value is kept and
    a field it lacks is filled in (papers' `keep`); *Keep library* and *Take
    all new* set every row. The tags (`keywords`), key and files of the paper
    are the library's and not compared (an edit keeps them). With a PDF: a
    paper without one has *Attach this PDF* (on), one with has *Also attach* /
    *Don't attach* (default); hidden without a PDF. There is no *Skip* / *Fill
    in* / *Update* here: the table and its two shortcuts replace them. With
    several entries, all of them papers of the .bib, the table is of the first
    and a note says that the others are kept as they are. A plain .bib has a
    link *Add it as a new paper* for a paper that only looks the same.
  - *The main button says what happens*, with a line next to it: *Add paper* (the
    PDF: moved, copied or linked); *Merge into KEY* (*2 fields, attach PDF*);
    *Attach PDF to KEY*; *Nothing to change*, disabled, when no row takes the new
    value and no PDF is attached. Writing, through the writer: the library's
    entry with the values taken (`merge_entry`; its key, tags, files and the other
    fields stay) is an edit (`replace_entry`: `papers edit KEY --from -`); with a
    PDF, `attach_pdf` (the edit, then `papers add --key KEY --attach PDF
    --move|--copy|--link`), as *A PDF for a paper that has none* below: a step
    of the history for each papers command, in the built-in writer and a papers
    command alike. Entries some of which are new are added by `add_bibtex` with
    `--on-duplicate skip`: the papers already there are kept as they are, and
    listed.
  - *Position and errors*: the sheet opens near the top of the window and grows
    downwards (it scrolls when taller than the window); the text of an error is
    selectable (it holds the link), and after a failed download *Open in
    browser* is the highlighted button (the app does not pretend to be a
    browser: no browser User-Agent).
  - *A journal article's link*: `journal_page` reads the DOI from the
    `citation_doi`, `prism.doi` and `dc.identifier` tags of the page
    (`papers_rs::fetch::html::page_doi`); with a DOI (typed or from the page)
    and no PDF, OpenAlex is asked for a free PDF (`free_pdf`,
    `Fetcher::openalex_free_pdf`: `best_oa_location.pdf_url`, else an open
    location, else `open_access.oa_url`): *Download free PDF* (`download_pdf`,
    into a temporary folder papers knows as a download, always placed in the
    library) and *Open in browser* (the article's page, to download there and
    drop the file).
  - *The file*: *Move*, *Copy*, *Leave in place* (papers' `--move`, `--copy`,
    `--link`), by default the `files` setting of the library (`default_files_mode`;
    move without one). papers 3.0.0b5 has no separate *rename*: `--rename` is
    `--move`, and a file placed in the files folder is always named by the
    library's name template, so there is no such checkbox. A plain `.bib` has no
    files folder: the PDF is linked in the `file` field where it is
    (`papers_core::with_files`), and the sheet says so; a downloaded file is
    refused there.
  - *Adding*: `add_bibtex` with the entry and `options.pdf`, `options.files`:
    `papers add ENTRY.bib --attach PDF --move|--copy|--link --on-duplicate …`
    (an entry that is a paper of the library is not added this way: see *The
    same paper* above). The paper is then selected, with its details.
  - *A PDF for a paper that has none*: the details have *Add PDF…* (and a click
    on the empty page, or a PDF dropped on it): the sheet opens on that paper's
    entry, its DOI in the field and the chooser of the PDF open. *Fetch* merges
    the fetched entry into the paper's text (*Fill in* or *Update*); the PDF's
    DOI is compared with the paper's and a difference told. `attach_pdf`: the
    changes of the text first (`papers edit KEY --from -`, its key, tags and
    files stay), then `papers add --key KEY --attach PDF --move|--copy|--link`
    (two steps of the history when both happen; a plain `.bib` makes one:
    `Library::attach_files`).
  - *Edit* in the details has the same *DOI* field and *Fetch*, merged into the
    text being edited (`merge_fetched`: papers' merge, the key stays), to review
    and save with ⌘S.
- **The command line tool**: the app ships `papers-rs` (crates/papers-rs-cli,
  release) as a Tauri sidecar (`bundle.externalBin`, `papers-rs-cli` in
  `Contents/MacOS/` next to the executable: tauri-build copies it into
  `target/<profile>/`, where `papers-rs` is cargo's own); `scripts/sidecar.sh`,
  run by `beforeBuildCommand`, builds it into
  `src-tauri/binaries/papers-rs-cli-<triple>` (`build.rs` puts a placeholder
  there for `cargo check` and `cargo test`). Settings (General)
  and the app menu (macOS: *Install Command Line Tool…*) install it as a link
  `papers-rs` → the binary in the bundle (`cli_tool.rs`) in `~/.local/bin`
  (macOS and Linux alike, created if missing; no password, no administrator
  prompt; not from an AppImage, which is mounted only while it runs). At
  start, the app points its link again if what it points to is gone (the app
  moved, or updated from 0.6, whose sidecar was `papers-rs`). If that
  folder is not in the PATH of the user's login shell (`$SHELL -lic`),
  Settings shows `export PATH="$HOME/.local/bin:$PATH"` and a button adding it
  to the startup file of the shell (zsh `~/.zshrc`; bash `~/.bashrc`, on macOS
  the profile it reads at login, `~/.bash_profile`; fish `fish_add_path` in
  `config.fish`; another shell: only the line) as a commented block, once and
  only on the click. The menu item says the same in an info dialog. Settings
  shows whether it is installed and where it points, with *Uninstall*; a file or
  link at the path that is not ours (a link to the app's `papers-rs`, whichever
  copy) is never replaced or removed. papers-rs is the same program as the
  built-in writer; papers (Python) can be installed alongside.
- **Any other `.bib`** is written by the app (`crates/core/src/write.rs`):
  only the edited entries are rewritten, the rest of the file stays byte for
  byte; the file is written to a temporary file then renamed, and not at all
  if it changed since it was read; the new text must load as the library
  reads it. Undo and redo: a stack of the changes of the session. Pasted
  entries whose key is in the file already are not added, but reported. A PDF
  added to an entry is linked in its `file` field where it is (JabRef's
  `:path:PDF`, the real path), not moved or renamed.
- **Delete, copy, move** (`remove_entries`, `transfer_entries`; both writers):
  - *Delete* removes the entries (`papers rm --keep-files`) and the files the
    user ticked (`remove_entries(ids, files)`), those that no other entry links
    to; they are deleted and kept in papers' history, so that undo brings them
    back (more than the history keeps, `history-size`, goes to the system
    trash). Built in, in one step of the history: `take_out` lists the files of
    the removed entries, those ticked are deleted through `Session::delete_files`
    (`assume_yes`: the app has asked already). A papers command has no command
    for some of the files of removed entries, so it is two steps, the files
    first (`papers file rm FILE... --key KEY...`, which removes the links of
    these entries and deletes the files nothing else links to), then the entries
    (`papers rm --keep-files`): undo twice brings both back. A move to another
    papers library deletes all the files of the moved entries the same way. A
    shallow library refuses to delete files (they are the pool's). A plain .bib:
    the entries are removed by the app's writer and the ticked files that no
    other entry links to are moved to the system Trash (recoverable by hand;
    never deleted for good: without a trash the files stay and the error says so).
  - *Copy to* / *Move to* another .bib: the entries are added by the writer of
    the target, as the entry is (tags and fields kept) without its `file`
    links, which are replaced by links to the files that are there (a link
    to a missing file is left out and counted). A papers target: one `papers add`
    of the entries with `--on-duplicate skip --copy`, so papers copies each PDF into
    its files folder, named by its template, in the same step. A plain .bib target:
    `Library::add_entries`, the PDFs linked where they are (real path). A paper the
    target has already, by key or, for papers, by its rule for the same paper, is
    not added, and not removed from the source: the notice says how many were
    already there (*Copied 2 papers to refs.bib, 1 already in refs.bib*).
  - *Move* is that copy followed by the removal of the papers added from their
    .bib: from a papers library to a papers library the PDFs go with them (papers
    never moves a file that belongs to another library, so they are copied, and
    the source's are deleted as in *Delete*: kept in the source's history, which
    undo brings back); the PDFs of a plain .bib source stay where they are, and so
    do those of a papers library moved to a plain .bib (the target links them).
    When the removal fails after the copy, the error says so and the library is
    read again.
  - Undo of a copy is one step of the target; of a move, one step of each .bib
    (the removal in the source, the addition in the target).
- **Undo and redo**: Edit → Undo and Redo (`⌘Z`, `⌘⇧Z`; Ctrl elsewhere): in a
  text field the field's own; otherwise the last change of a .bib file, through
  the commands `undo` and `redo` (papers' history for a papers library, the app's
  stack of the session for a plain .bib). The .bib is the one of the paper in
  front or shown, else of the library tab, else the only one; after a delete,
  copy or move, the one the app wrote until another paper is chosen. On macOS the
  menu items are the app's (`menu.rs`, which sends the field's undo to any other
  window); elsewhere the page catches the keys.
- **An edit of the BibTeX** of an entry keeps its key, tags and files, in
  either library, as `papers edit --from` does: an edit that changes them is
  refused. Tags have their own commands, files will too.
- Fields are saved with `⌘S`, not on every change: each papers write is one
  undo step and one git commit, and autosaving would write half-typed
  values. A tab with unsaved edits shows a dot, and closing it asks first.
- **Tags** are matched whole in the search: `tag:amoc` is that tag, `*` a
  wildcard (`tag:amoc*`, `tag:*amoc`); a plain word still finds tags
  loosely. A click on a tag in the sidebar adds it as a tag filter of the library tab (the core gets a `tag:x` term for it).
- **Nested tags**: a tag with `/` is under the tags before it:
  `climate/amoc` is under `climate`. Still one string in `keywords`, which
  papers writes as any other. `tag:climate` finds `climate` and the tags
  under it, not `climate-review`; `tag:climate/amoc` that tag and the ones
  under it. The sidebar shows the tags as a tree that folds, a parent
  counting the papers under it (a parent no paper has itself shows too);
  renaming a parent renames the tags under it, and the pinned tags and the
  search follow; a chip shows the whole tag, its parents dimmed.

**Done** (after 0.5.1, not released): exact tags; the writer
(`src-tauri/src/writer.rs`: the commands `writers`, `set_tags`,
`rename_tag`, `replace_entry`, `add_bibtex`, `remove_entries`, `undo`,
`redo`; errors `{ kind: "papers-missing" | "changed" | "failed", message }`;
`src/lib/library/writes.ts`); in the window, `write()` runs a write, says
its error or shows `PapersMissing.svelte`; Settings has the choice of the
writer of the papers libraries (built in, or a papers command with its path)
and tells how each `.bib` is written (`kind`: `papers-rs`, `papers`,
`native`, `papers-missing`). A papers library is edited as `papers edit KEY
--from -` does. Tags in the window: chips in the details (on
the paper, or the papers selected), the menu of a tag in the sidebar
(*Rename…* in place, *Pin to Key*), `1`–`9` toggle a pinned tag; nested
tags (the search, the tree, the rename of a parent, the chips). Tried in
the built app on a library of papers.

*Edit* in the details turns the BibTeX of the entry into a text to edit
(`⌘S` saves, `Esc` cancels; a change of key, tags or files is refused
under the text, `{ kind: "kept", what }`; unsaved changes ask before they
are lost, a dot on the paper's tab). *Add…* (`⌘N`): see *Add paper* above;
the papers already in the library listed first (`incoming`: the same DOI,
or the same title and year); for a papers library, what to do with them
(`--on-duplicate`: *Skip*, *Fill in*, *Update*), and a fetched entry takes
the library's key (`--update-key`); the `.bib` to add to when there are
several. The sheet as it was (a DOI fetched from doi.org, or BibTeX) was
tried in the built app, but for the choices on duplicates (tested against
papers in `writer.rs`); the import sheet is tested in `writer.rs` (both
backends, the command one against papers 3.0.0b5) and `fetch.rs` (recorded
answers), not yet tried in the built app.

**Next** (proposed, to confirm before starting; one agent each, in a git
worktree, merged one by one):

1. **Files**: detach a PDF (attaching one is done: *Add paper*), rename it by
   the names of papers (`papers file rename`, papers libraries only). (Delete,
   copy and move, and undo and redo, are done: *Delete, copy, move* above.)
2. **A manuscript's tag**, after these: the citations of a `.md` (`[@key]`),
   `.tex` or `.aux` tag their papers, and the keys not in the library are
   listed.

**Upstream** (perrette/papers, branch `v3`, not in a release yet): `edit
--from` (#117), safe writes (#119) and the smaller points (#121) are in
(pull request #123), with `--porcelain` and BibJSON (#118, pull requests
#125, #126), nested tags (#128) and the history without git (3.0.0b5).
To report: with `add --on-duplicate skip --porcelain`, a skipped entry has
no `existing` line (it is not returned as touched in
`_insert_entry_check`); and what the conformance suite found (below).

Towards the app's own writer for everything: papers-rs (below), once it
writes as papers does; then the app links it and needs no Python.

## papers-rs: a Rust port of papers

Decided with Mahé Perrette: a second implementation of papers, in Rust,
here for now, that can leave as its own repository. papers (Python) is the
truth; papers-rs does what it does, with the same output and the same
files, so that both (and the app) work on the same libraries and the same
history. In the long run a standard both follow and change together.

- `crates/papers-rs` (the library) and `crates/papers-rs-cli` (the command,
  `papers-rs`): no dependency on the app's crates. MIT, Mahé's copyright
  kept.
- `crates/latexenc-rs` (library `latexenc`): a port of pylatexenc 2.11 (the
  LaTeX parser, LaTeX to text, Unicode to LaTeX), with all its options; on
  its own, for anyone. Started from tex2word-latexwalker (yfyang86).
- `spec/`: the conformance suite. A case is a library before, commands, and
  what they print and leave after; recorded by running papers
  (`record.py`), checked on any implementation (`run.py --papers CMD`).
- **The standard**: Mahé's specification (perrette/papers, branch
  `design-spec`, `design/`: library, bibtex, formats, exchange, history)
  marks each rule *Contract* (files, keys, formats on disk: identical) or
  *Free* (messages, prompts, caches). The suite is its executable side: a
  case names its section and whether it checks a contract.

**Done**: follows papers v3 at 0941475 (3.0.0b7.dev3, the specification
merged in `design/` of perrette/papers): it passes all 225 offline cases
of the suite re-recorded from it (2026-10-29). Earlier: every command of papers 3.0.0b5 but `open`, `install`,
`uninstall`, `git` and Google Scholar (`--scholar`: scraping, left out),
pools and the exchanges included. The conformance suite has 218 cases:
papers and papers-rs pass all 212 offline ones in contract-only mode (the
full comparison differs only in Free output: bibtexparser's warnings, the
usage lines of `--openalex`). Interop both ways: each reads, undoes and
pulls what the other recorded. 3 to 8 times faster than papers
(`crates/papers-rs/PERFORMANCE.md`). The app writes with it, built in
(*Writing*). Decided: transliteration with AnyAscii (ISC; the `any_ascii` crate, papers'
own table since v3, perrette/papers#192: the output matches papers for every
code point), PDF text with `pdf-extract` (PyMuPDF has no
permissive Rust binding), the release profile unwinds (a panic of
pdf-extract is caught).

**Extension**: `--openalex` (add, extract, fetch), a full-text search on
OpenAlex's open API where papers has `--scholar`; off by default. On 300 of
the user's PDFs it finds 21 of the 38 (with text) that papers' chain
misses. To propose to Mahé.

**Upstream**: findings posted on perrette/papers (#188–#194), with the
offer of the suite and `--openalex` (#193). The way of working, agreed
with Mahé: `design/` is the basis; papers-rs reports what it finds there
as issues or pull requests, and catches up with each release.

**Next**: the crates leave for a repository of their own and are published
(crates.io: `latexenc-rs`, `papers-rs`, `papers-rs-cli`; prebuilt binaries
in releases, Homebrew), the app depending on the published crates.

## Later (P2)

Merge duplicates; checks (missing DOIs, broken links, keys); saved
searches; full-text search of the PDFs; git sync of the `.bib` files (not
the PDFs) with a remote; RIS and CSL-JSON import and export; annotations
kept in the PDF.

## Later: adding papers without a browser extension

Done in the main window (*Add paper*, above): a DOI or an article URL pasted
in the sheet, the metadata from the DOI (the `citation_doi` tags of the page),
the free PDF from OpenAlex, else the page in the browser and the PDF dropped
on the window. Left:

1. *Add current page* in the panel (Safari and Brave tell their current URL
   through AppleScript; paste on Linux and Windows).
2. The PDF when the institution's network gives access (not only open
   access); *armed*: the app waits for the next PDF in `~/Downloads`
   (downloaded in the browser, with its login), checks its DOI, and opens the
   sheet with it.

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
