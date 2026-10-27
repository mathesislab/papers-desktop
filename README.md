# papers-desktop

A global-hotkey search panel over a BibTeX library: one or more `.bib` files
and the PDFs they link to (JabRef-style `file` fields), as kept by
[papers](https://github.com/perrette/papers), JabRef, BibDesk or a Zotero
export.

The `.bib` files stay the only source of truth: the app reads them as they
are, and does not need papers or any other tool.

Built with [Tauri 2](https://tauri.app) (Rust) and SvelteKit (TypeScript).

Website and downloads: <https://mathesislab.github.io/papers-desktop/>. The
design and the plans (main window, editing, adding papers) are in
[docs/design.md](docs/design.md).

## Use

The app lives in the menu bar (no Dock icon). `⌥Space` (`Ctrl+Alt+Space`
elsewhere) shows the search panel; `↑`/`↓` move, `Esc` hides it. The
menu-bar icon has Open Library (also `⌘O` in the panel), Settings… (also
`⌘,`), Reload Library, and Send Feedback…, which opens a new GitHub issue.

| Key | Action |
|---|---|
| `↵` | open the PDF (the DOI page when there is none) |
| `⌘↵` | reveal the PDF in the Finder |
| `⌘D` | open the DOI page |
| `⌘⇧C` | copy the key |
| `⌘.` | copy `[@key]` |
| `⌘L` | copy `\cite{key}` |
| `⌘B` | copy the BibTeX entry |
| `⌘⌥C` | copy the citation, formatted |
| `⌘P` | show or hide the preview (first page, details, citation, BibTeX) |
| `⌘I` | open the paper in the library window |
| `⌘K` | list the actions |

`Ctrl` instead of `⌘` on Linux and Windows.

The library window (in the Dock while open) lists the papers in a table
that the same search filters, with the `.bib` files, views (recently opened,
no PDF, broken file links, possible duplicates) and tags in the sidebar.
Its columns sort, move, resize, and show or hide (right-click the headers).
Enter or a double-click opens a paper in a tab of the sidebar: on macOS the
PDF shows as in Preview (PDFKit), elsewhere with pdf.js. Export… writes the
selection, or the papers shown, to a `.bib` file.

| Key (library window) | Action |
|---|---|
| `/`, `⌘F` | search the library, or find in the PDF |
| `↑` `↓`, `⇟` `⇞`, `↖` `↘` (`⌘↑` `⌘↓`) | move; with `⇧`, extend the selection |
| `↵` | open the paper |
| `Esc` | clear the search, then leave it |
| `⌘W` | close the paper (the window, from the library) |
| `⌘1`, `⌃Tab`, `⌃⇧Tab` | the library, the next tab, the one before |
| `⌘⌥S`, `⌘⌥I` | show or hide the sidebar, the details |
| `⌘=` `⌘-` `⌘0` | zoom in, out, fit the width |

Drag a paper out of the list to drop its PDF in the Finder, a mail or a
chat, and the citation out of the preview to drop it, formatted, in a
document.

The empty panel lists the papers opened or copied last (Settings clears
them), above a reminder of the keys and filters.

In the preview, *Share* copies a snippet (the title and its link), the
link, the DOI, the citation or its HTML; the BibTeX shows formatted, or raw
as written in its `.bib` file. The citation and the snippet paste formatted
(the title as a link) where formatting is possible, as plain text
elsewhere.

The library reloads by itself when a `.bib` file changes (papers, JabRef,
an editor, a sync client).

Every word must match an author, the year, key, title, a keyword, the
journal or the DOI, from the start of a word ("rob" finds Robinson). The
matches are listed newest first, with the matched words in bold. A DOI or a
doi.org link finds its entry.

Paste a reference (a DOI in it, or six words or more) to find the entries
closest to it: the entry of its DOI, else those with most of its words,
the names counting most. Text selected in another app is found the same
way: on macOS, right-click it → Services → Search Papers (System Settings →
Keyboard → Keyboard Shortcuts → Services → Text gives it a shortcut); on
Linux, press `Ctrl+Alt+Shift+Space`, or run `papers-desktop --selection`
from a desktop shortcut under Wayland ([below](#linux)).

Filters limit a word to one field, and quotes match a phrase:

| Filter | Short | Matches |
|---|---|---|
| `author:robinson` | `a:` | any author |
| `first:robinson` | `f:` | the first author |
| `title:hysteresis` | `t:` | the title |
| `year:2012` | `y:` | the year: `2010-2015`, `2020-`, `-1990`, or its first digits (`201`) |
| `journal:cryosphere` | `j:` | the journal or book |
| `key:robinson2012` | `k:` | the citation key |
| `doi:10.5194` | | the DOI |
| `tag:greenland` | | the keywords |
| `has:pdf`, `has:abstract` | | a linked file, or any field of the entry |
| `"ice sheet"`, `t:"ice sheet"` | | consecutive words |

A leading `-` excludes the papers that match a term: `-review`,
`-a:smith`, `-y:2012`, `-has:pdf`.

The year of each paper stands in a column on the left. Clicking a year
keeps that year only (click again to undo), `⇧`-click stretches the year
range to it, and `⌥`-click excludes it. Above the list, a strip shows the
matches of each year (before the year filters): drag across it to keep a
range of years, or click it like a year; All years ✕ drops the year
filters.

## Settings

The settings window (menu-bar icon → Settings…, or `⌘,` in the panel) sets
the `.bib` files of the library, the hotkey (click it and press the new
keys) and, on Linux, the hotkey that searches the selection, matching
inside words, the citation style, Start at Login and updates.
Changes apply at once.

They are kept in `settings.json` in the app config directory
(`~/Library/Application Support/io.github.alex-robinson.papers-desktop/` on
macOS, [the Linux path is below](#linux)), which can also be edited:

```json
{
  "bibtex": ["~/lit/papers.bib"],
  "hotkey": "Alt+Space",
  "matching": "word-start",
  "citation_style": "copernicus-publications"
}
```

- `bibtex`: the `.bib` files of the library. When none is set, the app takes the default
  library of papers, if papers is set up. Relative links to files are
  relative to their `.bib` file; a key in two files gets a number
  (`smith2010#2`).
- `hotkey`: `Alt+Space` on macOS, `Ctrl+Alt+Space` elsewhere. Edited in the
  file, it applies at the next start.
- `selection_hotkey` (Linux): `Ctrl+Alt+Shift+Space`, searches the selected
  text.
- `matching`: `"word-start"` (default), or `"inside-words"` to also match
  inside words from three letters ("stab" finds Multistability).
- `citation_style`: the style of the citation in the preview, one of the
  bundled [CSL](https://citationstyles.org) styles —
  `copernicus-publications` (default), `american-geophysical-union`,
  `american-meteorological-society`, `nature`, `science`, `apa`,
  `elsevier-harvard`, `chicago-author-date` — or the path of any `.csl` file
  (e.g. from the [Zotero style repository](https://www.zotero.org/styles)).

Reload Library re-reads the file and the library.

## Install

From the [GitHub release](https://github.com/mathesislab/papers-desktop/releases):

- macOS (Apple Silicon): the `.dmg`. The app is not notarized: on first
  launch, allow it in System Settings → Privacy & Security (*Open Anyway*).
- Ubuntu: see [Linux](#linux).

Then choose the `.bib` files of your library: menu-bar icon → *Settings…*.

## Requirements (to build)

- Rust (stable) and Node.js

## Develop

```bash
npm install
npm run tauri dev
```

## Updates

The app looks for a new release at start and twice a day (a switch in the
settings turns this off), and the menu-bar icon then shows *Update to …*. It
installs only when asked: Settings → Install and Restart. The new version
then opens the settings at Updates, with a link to the release notes
(What's new). On Linux the
`.deb` installs with the system's password prompt; the AppImage replaces
itself.

The updates are signed with the updater key (`~/.tauri/papers-desktop.key`,
its public key in `src-tauri/tauri.conf.json`). The release workflow signs
with the `TAURI_SIGNING_PRIVATE_KEY` secret of the repository and uploads
`latest.json`, which the apps read from the latest release. Keep a copy of
the key: without it, the apps installed cannot take a new version.

## Build

```bash
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/papers-desktop.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
  npm run tauri build -- --bundles app
```

Without the key, build without the signed update archive:
`npm run tauri build -- --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'`.

The app is then `target/release/bundle/macos/Papers Desktop.app`: copy it to
`/Applications` before turning on Start at Login, which points at the app
where it is.

## Linux

On Ubuntu, from the [GitHub release](https://github.com/mathesislab/papers-desktop/releases):

- the `.deb`: `sudo apt install ./Papers-Desktop-linux-amd64.deb` (installs the
  `papers-desktop` command), or
- the AppImage: `chmod +x` it, then run it.

The default hotkey is `Ctrl+Alt+Space`, and `Ctrl+Alt+Shift+Space` searches
the selected text (the primary selection). Under Wayland (the default of
Ubuntu) an app cannot grab a global hotkey: add a shortcut in Settings →
Keyboard → View and Customize Shortcuts → Custom Shortcuts, with the command
`papers-desktop` (or the path of the AppImage). It toggles the panel of the
running app. Another shortcut with `papers-desktop --selection` searches
the selected text.

The menu-bar (tray) icon needs AppIndicator support. Ubuntu ships it; on other
GNOME desktops install the "AppIndicator and KStatusNotifierItem Support"
extension. Without the icon the hotkey and the settings file still work.

The settings file is
`~/.config/io.github.alex-robinson.papers-desktop/settings.json`.

To build from source:

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

## License

MIT. The bundled citation styles are CC BY-SA 3.0 (see
[src-tauri/styles](src-tauri/styles/README.md)).
