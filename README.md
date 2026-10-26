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

Every word must match the author, year, key, title, keyword, journal or DOI;
first authors, keys and exact words rank first. A DOI or a doi.org link
finds its entry.

## Settings

`settings.json` in the app config directory
(`~/Library/Application Support/io.github.alex-robinson.papers-desktop/` on
macOS), written on first run:

```json
{
  "papers": "/path/to/bin/papers",
  "hotkey": "Alt+Space"
}
```

- `papers`: the papers executable. Apps started from the Finder or at login
  do not see the shell `PATH`, so the app looks it up once in your login
  shell; set it here if that fails.
- `hotkey`: read at startup only (quit and restart after a change).

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
