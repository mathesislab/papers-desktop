#!/usr/bin/env bash
# Smoke test of the debug app on Linux, under xvfb and a private D-Bus, with
# the test library: the app must stay up for a while, without a panic, and
# load the library.
set -euo pipefail

seconds=20
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$root"

cargo build -p papers-desktop

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# xvfb-run merges the error output of the app into its output
log=$work/app.log
cp crates/core/tests/fixtures/library.bib "$work/library.bib"

# a temporary home keeps the settings of the runner (and of a developer) out
home=$work/home
config=$home/.config/io.github.alex-robinson.papers-desktop
mkdir -p "$config"
printf '{"bibtex": ["%s"], "hotkey": "Ctrl+Alt+Space"}\n' "$work/library.bib" > "$config/settings.json"

# the app runs until the timeout stops it (124): a crash ends it earlier
status=0
env HOME="$home" XDG_CONFIG_HOME="$home/.config" WEBKIT_DISABLE_DMABUF_RENDERER=1 \
  dbus-run-session -- xvfb-run -a timeout "$seconds" target/debug/papers-desktop \
  > "$log" 2>&1 || status=$?

failures=()
[[ $status -eq 124 ]] || failures+=("exit status $status, expected 124 (still running after ${seconds}s)")
! grep -q panicked "$log" || failures+=("the app panicked")
grep -q "loaded 8 entries from 1 .bib files" "$log" || failures+=("the library was not loaded")

if ((${#failures[@]})); then
  printf 'smoke test failed: %s\n' "${failures[@]}" >&2
  echo "--- output of the app ---" >&2
  cat "$log" >&2
  exit 1
fi
echo "smoke test passed: app ran for ${seconds}s and loaded the library"
