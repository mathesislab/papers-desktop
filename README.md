# papers-desktop

Desktop companion for the [papers](https://github.com/perrette/papers) CLI:
a global-hotkey search panel over your papers library.

The `.bib` file and the PDF folder managed by `papers` stay the only source
of truth; the app reads them through `papers list --format csl-json`.

Built with [Tauri 2](https://tauri.app) (Rust) and SvelteKit (TypeScript).

## Use

The app lives in the menu bar (no Dock icon). `⌥Space` shows the search
panel; `↑`/`↓` move, `Esc` hides it. The menu-bar icon has Reload Library,
Settings… and Start at Login.

| Key | Action |
|---|---|
| `↵` | open the PDF (the DOI page when there is none) |
| `⌘↵` | reveal the PDF in the Finder |
| `⌘D` | open the DOI page |
| `⌘⇧C` | copy the key |
| `⌘.` | copy `[@key]` |
| `⌘L` | copy `\cite{key}` |
| `⌘B` | copy the BibTeX entry |
| `⌘P` | show or hide the preview (first page, details, BibTeX) |
| `⌘K` | list the actions |

`Ctrl` instead of `⌘` on Linux and Windows.

The library reloads by itself when the bibtex file changes (a `papers`
command, an editor, a sync client).

Every word must match an author, the year, key, title, a keyword, the
journal or the DOI, from the start of a word ("rob" finds Robinson). The
matches are listed newest first, with the matched words in bold. A DOI or a
doi.org link finds its entry.

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
| `"ice sheet"`, `t:"ice sheet"` | | consecutive words |

A leading `-` excludes the papers that match a term: `-review`,
`-a:smith`, `-y:2012`.

The year of each paper stands in a column on the left. Clicking a year
keeps that year only (click again to undo), `⇧`-click stretches the year
range to it, and `⌥`-click excludes it. Above the list, a strip shows the
matches of each year (before the year filters): drag across it to keep a
range of years, or click it like a year.

## Settings

`settings.json` in the app config directory
(`~/Library/Application Support/io.github.alex-robinson.papers-desktop/` on
macOS), written on first run:

```json
{
  "papers": "/path/to/bin/papers",
  "hotkey": "Alt+Space",
  "matching": "word-start"
}
```

- `papers`: the papers executable. Apps started from the Finder or at login
  do not see the shell `PATH`, so the app looks it up once in your login
  shell; set it here if that fails.
- `hotkey`: read at startup only (quit and restart after a change).
- `matching`: `"word-start"` (default), or `"inside-words"` to also match
  inside words from three letters ("stab" finds Multistability).

Reload Library re-reads the settings and the library.

## Requirements

- papers-cli 3.0 (`pip install --pre papers-cli`), with a default library
- Rust (stable) and Node.js

## Develop

```bash
npm install
npm run tauri dev
```

## Build

```bash
npm run tauri build
```
