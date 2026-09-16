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

# Put the bundled voicectl on PATH so scripts and agents can edit snippets.
# A symlink (not a copy) keeps it in lockstep with the installed app. Prefer
# a bin dir that is already on PATH and writable without sudo.
CLI_SRC="$DEST/Contents/MacOS/voicectl"
CLI_LINK=""
for BIN_DIR in /opt/homebrew/bin /usr/local/bin; do
    case ":$PATH:" in
        *":$BIN_DIR:"*) ;;
        *) continue ;;
    esac
    if [ -d "$BIN_DIR" ] && [ -w "$BIN_DIR" ]; then
        ln -sfn "$CLI_SRC" "$BIN_DIR/voicectl"
        CLI_LINK="$BIN_DIR/voicectl"
        break
    fi
done

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

if [ -n "$CLI_LINK" ]; then
    printf '\nThe voicectl command is installed at %s\n' "$CLI_LINK"
    printf 'Try:  voicectl snippets add brb "be right back"\n'
else
    printf '\nTo use the voicectl command (edit snippets from a script), link it onto PATH:\n'
    printf '  sudo ln -sfn "%s" /usr/local/bin/voicectl\n' "$CLI_SRC"
fi
