#!/usr/bin/env bash
# Smoke test of the debug app on Linux, under xvfb and a private D-Bus, with a
# fake `papers`: the app must stay up for a while, without a panic, and ask
# papers for the library and for the bibtex file to watch.
set -euo pipefail

seconds=20
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$root"

cargo build -p papers-desktop

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
log=$work/papers.log
stderr=$work/stderr.log

# the fake papers writes its arguments to papers.log, next to itself
cp crates/core/tests/fixtures/library.json "$work/library.json"
: > "$work/papers.bib"
: > "$log"
cat > "$work/papers" <<'EOF'
#!/usr/bin/env bash
dir=$(cd "$(dirname "$0")" && pwd)
echo "$*" >> "$dir/papers.log"
case "$*" in
  "list --format csl-json") cat "$dir/library.json" ;;
  "config bibtex") echo "$dir/papers.bib" ;;
esac
EOF
chmod +x "$work/papers"

# a temporary home keeps the settings of the runner (and of a developer) out
home=$work/home
config=$home/.config/io.github.alex-robinson.papers-desktop
mkdir -p "$config"
printf '{"papers": "%s", "hotkey": "Ctrl+Alt+Space"}\n' "$work/papers" > "$config/settings.json"

# the app runs until the timeout stops it (124): a crash ends it earlier
status=0
env HOME="$home" XDG_CONFIG_HOME="$home/.config" WEBKIT_DISABLE_DMABUF_RENDERER=1 \
  dbus-run-session -- xvfb-run -a timeout "$seconds" target/debug/papers-desktop \
  2> "$stderr" || status=$?

failures=()
[[ $status -eq 124 ]] || failures+=("exit status $status, expected 124 (still running after ${seconds}s)")
! grep -q panicked "$stderr" || failures+=("the app panicked")
for call in "list --format csl-json" "config bibtex"; do
  grep -qxF "$call" "$log" || failures+=("papers was not called with: $call")
done

if ((${#failures[@]})); then
  printf 'smoke test failed: %s\n' "${failures[@]}" >&2
  echo "--- papers calls ---" >&2
  cat "$log" >&2
  echo "--- stderr ---" >&2
  cat "$stderr" >&2
  exit 1
fi
echo "smoke test passed: app ran for ${seconds}s and loaded the library"
