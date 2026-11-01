# Sync (frozen)

**Status: frozen, nothing of it is built.** To be discussed with papers
(Mahé): papers 3 keeps its own history and dropped git, and part of this plan
needs papers to agree. The onboarding and the settings are built so that it
can be added later.

## Aim

You don't lose your .bib files and your settings when you move to a new
machine, and two machines share them. The PDFs and other heavy files are not
synced: they stay where they are, and can be fetched again or copied by hand,
or a synced drive can hold them.

## What is synced

- **The repository:** the *Papers folder*, which is the folder of the main
  papers library (`papers.bib` and `.papers/` at its root, `~/Papers` by
  default). Its location is set once and kept, and the app never moves it.
- **Tracked:**
  - the .bib files under the Papers folder;
  - `.papers/config.json`;
  - the settings that are not tied to one machine, in `.papers-desktop/`:
    feed subscriptions, interests and rules, saved views, the search services
    and the citation style.
- **Not tracked:**
  - PDFs (the PDF folder can be anywhere, inside the Papers folder or not);
  - `.papers/backup` and the rest of papers' history, since git keeps the
    history;
  - the feeds' state (items, seen, starred);
  - the settings of the machine: hotkeys, windows, the Downloads folder, the
    OpenAlex key, the PDF folder.
- **Outside the Papers folder:** .bib files elsewhere (a manuscript's
  `references.bib` in its own repository) stay in the library, but are not
  synced.

## How

- **Git:** the system `git`, with your own SSH keys or credential helper, so
  the app keeps no account. Any remote works: GitHub, GitLab, Gitea, or a
  bare repository on a server.
- **When:**
  - a commit after a change, debounced;
  - a pull at start and every N minutes;
  - a push after a commit.
- **Conflicts:**
  - .bib files are merged three ways by entry, with papers-rs's merge (the
    code of pools);
  - JSON settings are merged by field, and the newer side wins for a single
    value.
- **The Sync tab** of the settings: the remote, the last sync and its state,
  and *Sync Now*.

## What it needs first

1. **The settings in two parts:** those of the machine, which stay in the
   app's data folder, and the portable ones, which move to
   `.papers-desktop/` in the Papers folder.
2. **The PDF folder per machine:** today it is the library's `filesdir` in
   `.papers/config.json`, and so the same everywhere. papers would need a
   machine-level `filesdir` for a library (by its `id`, in its global
   config), so that the `papers` command and the app agree on it. **This is
   for papers to decide.**
3. **Relative paths in libraries** (`paths`), so that a .bib's `file` fields
   hold on another machine.
