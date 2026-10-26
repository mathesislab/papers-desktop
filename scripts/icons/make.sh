#!/usr/bin/env bash
# The app icons, from the logo of the website (macOS only: the SVG is drawn by AppKit).
# The logo takes 824 of 1024 pixels, the grid of the icons of macOS.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
swift "$root/scripts/icons/render.swift" "$root/site/images/logo.svg" "$work/icon.png" 1024 100
cd "$root"
npm run tauri icon -- "$work/icon.png"
# no mobile app
rm -rf src-tauri/icons/android src-tauri/icons/ios
