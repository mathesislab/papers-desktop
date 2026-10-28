#!/bin/sh
# Build the papers-rs command (crates/papers-rs-cli, release) and put it where
# Tauri's bundle.externalBin looks for it: src-tauri/binaries/papers-rs-<target
# triple>. Run by `beforeBuildCommand`, so `npx tauri build` ships it in the app.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
# Tauri tells the target of the build; else the one of this machine
triple=${TAURI_ENV_TARGET_TRIPLE:-$(rustc -vV | sed -n 's/^host: //p')}

if [ "${TAURI_ENV_TARGET_TRIPLE:-}" = "" ]; then
  cargo build --release -p papers-rs-cli --manifest-path "$root/Cargo.toml"
  built="$root/target/release/papers-rs"
else
  cargo build --release -p papers-rs-cli --manifest-path "$root/Cargo.toml" --target "$triple"
  built="$root/target/$triple/release/papers-rs"
fi

mkdir -p "$root/src-tauri/binaries"
cp "$built" "$root/src-tauri/binaries/papers-rs-$triple"
echo "sidecar: papers-rs-$triple"
