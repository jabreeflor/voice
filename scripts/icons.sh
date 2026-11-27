#!/usr/bin/env bash
# Regenerate every icon asset from the SVG sources in crates/voice-app/icons/.
# Edit the SVGs, never the PNG/ICNS/ICO outputs. Needs the Tauri CLI
# (`cargo install tauri-cli --version "^2" --locked`); its `icon` command
# rasterises SVG the same way on every OS, so the output is reproducible.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
icons="$root/crates/voice-app/icons"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

render() { # <svg> <out dir> [--png <size>]
  local log
  if ! log="$(cargo tauri icon "$1" -o "$2" "${@:3}" 2>&1)"; then
    echo "$log" >&2
    exit 1
  fi
}

# App icon: Dock / taskbar / app switcher / launcher / installers.
# tauri.conf.json `bundle.icon` lists exactly these files.
render "$icons/icon.svg" "$tmp/app"
for f in 32x32.png 128x128.png 128x128@2x.png icon.icns icon.ico icon.png; do
  cp "$tmp/app/$f" "$icons/$f"
done

# README image.
render "$icons/icon.svg" "$tmp/readme" --png 1024
cp "$tmp/readme/1024x1024.png" "$root/assets/icon_1024.png"

# Tray: 44 px = 22 pt @2x on macOS (the menu bar scales it down on 1x).
render "$icons/tray.svg" "$tmp/tray" --png 44
cp "$tmp/tray/44x44.png" "$icons/tray.png"
render "$icons/tray-recording.svg" "$tmp/rec" --png 44
cp "$tmp/rec/44x44.png" "$icons/tray-recording.png"
# Windows/Linux tray (no template images): 64 px covers 16-32 px at 200 %.
render "$icons/tray-color.svg" "$tmp/color" --png 64
cp "$tmp/color/64x64.png" "$icons/tray-color.png"

echo "icons regenerated in $icons"
