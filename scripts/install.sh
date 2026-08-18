#!/bin/bash
# voice installer — free, 100% local voice dictation for macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/jabreeflor/voice/main/scripts/install.sh | bash
#
# Builds from source on your machine (takes ~30 seconds), so there is no
# Gatekeeper "unidentified developer" friction and nothing to notarize.
set -euo pipefail

REPO_URL="https://github.com/jabreeflor/voice.git"
APP="Voice.app"
DEST="/Applications/$APP"

bold() { printf '\n\033[1m%s\033[0m\n' "$*"; }
fail() { printf '\033[31mError:\033[0m %s\n' "$*" >&2; exit 1; }

[ "$(uname)" = "Darwin" ] || fail "voice only runs on macOS."

MACOS_MAJOR=$(sw_vers -productVersion | cut -d. -f1)
[ "$MACOS_MAJOR" -ge 13 ] || fail "macOS 13 (Ventura) or later is required — you have $(sw_vers -productVersion)."

if ! xcode-select -p >/dev/null 2>&1 || ! command -v swift >/dev/null 2>&1; then
    fail "The Xcode Command Line Tools are required to build voice.
Run:  xcode-select --install
then re-run this script once the install finishes."
fi

if ! command -v brew >/dev/null 2>&1; then
    fail "Homebrew is required (it provides the whisper.cpp engine).
Install it from https://brew.sh then re-run this script."
fi

if ! command -v whisper-server >/dev/null 2>&1 \
   && ! [ -x /opt/homebrew/bin/whisper-server ] && ! [ -x /usr/local/bin/whisper-server ]; then
    bold "Installing whisper-cpp (the local transcription engine)..."
    brew install whisper-cpp
fi

WORKDIR=$(mktemp -d /tmp/voice-install.XXXXXX)
trap 'rm -rf "$WORKDIR"' EXIT

bold "Downloading voice..."
git clone --quiet --depth 1 "$REPO_URL" "$WORKDIR/voice"

bold "Building (this takes about 30 seconds)..."
(cd "$WORKDIR/voice" && ./build.sh)

bold "Installing to $DEST..."
# Quit a running copy so the bundle can be replaced cleanly.
osascript -e 'tell application "Voice" to quit' >/dev/null 2>&1 || true
pkill -x Voice 2>/dev/null || true
rm -rf "$DEST"
cp -R "$WORKDIR/voice/$APP" "$DEST"

open "$DEST"

bold "Done! voice is running in your menu bar (waveform icon)."
cat <<'EOF'

Two one-time permission grants and you're dictating:

  1. Microphone — approve the popup that just appeared.
  2. Accessibility — System Settings → Privacy & Security → Accessibility,
     toggle Voice on. (Needed to detect the hotkey and type text for you.)

On first launch the app downloads a small speech model (~142 MB) in the
background — the menu bar icon shows progress.

Then click into any text field, hold Right Option (⌥), speak, release.
Your words appear at the cursor. Esc cancels a recording.
EOF
