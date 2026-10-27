# Testing the app on macOS

The core is tested with `cargo test`; the windows are tried in the built
app, driven from a terminal with the scripts of `scripts/macos` (run with
`swift scripts/macos/NAME.swift ARGS`).

## A build in place of the installed app

```sh
npx tauri build --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'
pkill -f '/Applications/Papers Desktop.app/Contents/MacOS/papers-desktop'
target/release/bundle/macos/Papers\ Desktop.app/Contents/MacOS/papers-desktop &
```

The app is single instance: a second launch of the binary shows the panel
of the one running (`open` of a second copy does the same), so the
installed app must be quit first. It shares the data folder of the installed
app (`~/Library/Application Support/io.github.alex-robinson.papers-desktop`:
`window.json`, `recent.json`, the note `main-window-open`,
`.window-state.json`): copy them aside before a test that changes them, and
put them back after, with the installed app started again.

## Driving it

macOS does not let an app take the focus from the one in front unless the
user acts, so a build started from a terminal stays behind:

- `post.swift PID key CODE [cmd] [shift] [alt] [ctrl]`, `post.swift PID type
  TEXT`: keys sent to the process only, whatever is in front. They reach the
  key window of the app, and its menu (⌘W, ⌘F, ⌘1…). Clicks sent so are
  ignored by an app that is not active.
- `click.swift X Y`: a real click (the mouse moves). Only when the user is
  away (`ioreg -c IOHIDSystem`, `HIDIdleTime`) and on a window of the app
  in front at that point (`order.swift X Y` lists the windows there, front
  first): one click on the title bar makes the app active, and then
  `osascript -e 'tell application "System Events" to keystroke …'` and
  clicks reach it.
- `window.swift` prints the window number of the panel, `bounds.swift TITLE`
  the number and bounds of a window ("Papers" for the main window, "Papers
  Desktop Settings"); `screencapture -o -l NUMBER file.png` takes it, also
  behind other windows.
- `service.swift TEXT` sends a text to the "Search Papers" service.

## What a script cannot try

The menu-bar icon cannot be clicked from a script. The PDF view of PDFKit
lies over the web view: a screenshot shows it, `get_page_text` of a browser
does not.
