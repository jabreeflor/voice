#!/bin/bash
# Build Voice.app on macOS — local, free voice dictation.
#
# Convenience wrapper around `cargo tauri build`: the bundled app lands at
# target/release/bundle/macos/Voice.app, this script copies it to the repo
# root (where scripts/install.sh and scripts/e2e-smoke.sh expect it), adds
# voicectl to the bundle, and signs it.
set -euo pipefail
cd "$(dirname "$0")"

[ "$(uname)" = "Darwin" ] || { echo "build.sh is macOS-only; run 'cargo tauri build' in crates/voice-app instead." >&2; exit 1; }

APP="Voice.app"
# Respect a caller's CARGO_TARGET_DIR (agents build into per-worktree dirs).
TARGET="${CARGO_TARGET_DIR:-$PWD/target}"
BUNDLED="$TARGET/release/bundle/macos/$APP"

if ! cargo tauri --version >/dev/null 2>&1; then
    echo "Installing tauri-cli…"
    cargo install tauri-cli --version "^2" --locked
fi

echo "Compiling…"
# Only the .app is needed here; the .dmg from the default target list is
# what CI publishes and takes an extra minute per build.
(cd crates/voice-app && cargo tauri build --bundles app)
# `cargo tauri build` builds the app crate only; voicectl is its own package.
cargo build --release -p voicectl

rm -rf "$APP"
cp -R "$BUNDLED" "$APP"
# voicectl (command-line snippet editor) ships inside the bundle so it is
# always the same version as the app. scripts/install.sh symlinks it onto PATH.
cp "$TARGET/release/voicectl" "$APP/Contents/MacOS/voicectl"

# Sign with a real identity when one exists — a stable signature means macOS
# keeps the Accessibility/Microphone grants across rebuilds. Ad-hoc fallback.
IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null \
    | sed -n 's/^ *[0-9]*) [0-9A-F]* "\(.*\)"$/\1/p' | head -1)
# The nested voicectl binary is signed first; signing the bundle afterwards
# seals it into the app's code resources (adding a file to the bundle also
# invalidated whatever signature tauri-bundler applied, so re-signing is
# required regardless of identity).
if [ -n "$IDENTITY" ]; then
    echo "Signing with: $IDENTITY"
    codesign --force --sign "$IDENTITY" "$APP/Contents/MacOS/voicectl"
    codesign --force --sign "$IDENTITY" "$APP"
else
    echo "Signing ad-hoc (permissions will need re-granting after each rebuild)"
    codesign --force --sign - "$APP/Contents/MacOS/voicectl"
    codesign --force --sign - "$APP"
fi

echo "Built $APP"
echo "Run with:  open $APP"
echo "CLI:       $APP/Contents/MacOS/voicectl --help"
