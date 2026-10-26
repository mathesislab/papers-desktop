# papers-desktop

Desktop companion for the [papers](https://github.com/perrette/papers) CLI:
a global-hotkey search panel over your papers library.

The `.bib` file and the PDF folder managed by `papers` stay the only source
of truth; the app reads them through `papers list --format csl-json`.

Built with [Tauri 2](https://tauri.app) (Rust) and SvelteKit (TypeScript).

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
