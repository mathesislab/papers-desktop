#!/bin/sh
# Build the papers-rs command (papers-rs-cli of github.com/mathesislab/papers-rs, release) and put it where
# Tauri's bundle.externalBin looks for it: src-tauri/binaries/papers-rs-cli-<target triple>. Run by
# `beforeBuildCommand`, so `npx tauri build` ships it in the app. The command is that of the papers-rs the app is
# built with: the revision of Cargo.lock (`cargo build -p` builds no binary of a dependency, `cargo install` does).
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
# Tauri tells the target of the build; else the one of this machine
triple=${TAURI_ENV_TARGET_TRIPLE:-$(rustc -vV | sed -n 's/^host: //p')}

# the revision of papers-rs in Cargo.lock: its source, after the #
rev=$(awk '/^name = "papers-rs"$/ { found = 1 } found && /^source = / { print; exit }' "$root/Cargo.lock" |
  sed -n 's/.*#\([0-9a-f]*\)".*/\1/p')
if [ -z "$rev" ]; then
  echo "sidecar: no revision of papers-rs in Cargo.lock" >&2
  exit 1
fi

installed="$root/target/sidecar"
if [ "${TAURI_ENV_TARGET_TRIPLE:-}" = "" ]; then
  cargo install --git https://github.com/mathesislab/papers-rs --rev "$rev" --locked papers-rs-cli --root "$installed"
else
  cargo install --git https://github.com/mathesislab/papers-rs --rev "$rev" --locked papers-rs-cli --root "$installed" \
    --target "$triple"
fi

mkdir -p "$root/src-tauri/binaries"
cp "$installed/bin/papers-rs" "$root/src-tauri/binaries/papers-rs-cli-$triple"
echo "sidecar: papers-rs-cli-$triple ($rev)"
