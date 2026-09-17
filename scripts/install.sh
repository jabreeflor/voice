#!/bin/bash
# voice installer - free, 100% local voice dictation for macOS and Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/jabreeflor/voice/main/scripts/install.sh | bash
#
# Builds from source on your machine (a few minutes the first time, Rust
# compiles are slow), so there is no Gatekeeper "unidentified developer"
# friction on macOS and nothing to notarize. Windows users: see README.md
# for prebuilt bundles.
#
# Must stay ASCII-only and bash 3.2 compatible (macOS ships bash 3.2).
set -euo pipefail

REPO_URL="https://github.com/jabreeflor/voice.git"
APP="Voice.app"
DEST="/Applications/$APP"

bold() { printf '\n\033[1m%s\033[0m\n' "$*"; }
fail() { printf '\033[31mError:\033[0m %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

OS=$(uname)
case "$OS" in
    Darwin|Linux) ;;
    *) fail "This script supports macOS and Linux. On Windows, download a prebuilt bundle
(see README.md) or build from source with 'cargo tauri build'." ;;
esac

# ---------------------------------------------------------------------------
# Toolchain

have git || fail "git is required. Install it and re-run this script."

if ! have cargo; then
    fail "Rust is required to build voice.
Install it from https://rustup.rs (one command), open a new terminal, then re-run this script."
fi

if [ "$OS" = "Darwin" ]; then
    MACOS_MAJOR=$(sw_vers -productVersion | cut -d. -f1)
    [ "$MACOS_MAJOR" -ge 13 ] || fail "macOS 13 (Ventura) or later is required - you have $(sw_vers -productVersion)."

    # The Rust linker and codesign come from the Command Line Tools.
    if ! xcode-select -p >/dev/null 2>&1; then
        fail "The Xcode Command Line Tools are required to build voice.
Run:  xcode-select --install
then re-run this script once the install finishes."
    fi

    have brew || fail "Homebrew is required (it provides the whisper.cpp engine).
Install it from https://brew.sh then re-run this script."

    if ! have whisper-server \
       && ! [ -x /opt/homebrew/bin/whisper-server ] && ! [ -x /usr/local/bin/whisper-server ]; then
        bold "Installing whisper-cpp (the local transcription engine)..."
        brew install whisper-cpp
    fi
else
    # Voice searches PATH plus a few fixed prefixes (see engine.rs); the
    # installer only needs to know one of them has the server.
    if ! have whisper-server && ! [ -x "$HOME/.local/bin/whisper-server" ] \
       && ! [ -x /usr/local/bin/whisper-server ]; then
        fail "whisper-server (from whisper.cpp) is required but was not found on PATH,
in ~/.local/bin or /usr/local/bin.

Install your distribution's whisper.cpp package if it ships whisper-server,
or build it from source:

  git clone --depth 1 --branch v1.9.4 https://github.com/ggml-org/whisper.cpp.git
  cmake -S whisper.cpp -B whisper.cpp/build -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF
  cmake --build whisper.cpp/build --config Release -j
  install -m 755 whisper.cpp/build/bin/whisper-server ~/.local/bin/

then re-run this script."
    fi

    # Tauri links against WebKitGTK/GTK; the hotkey, paste and audio crates
    # need X11, libxdo and ALSA headers. pkg-config answers for most of them;
    # libxdo has no .pc file so its header is checked directly.
    MISSING=""
    if have pkg-config; then
        for PKG in webkit2gtk-4.1 gtk+-3.0 ayatana-appindicator3-0.1 librsvg-2.0 alsa xtst xkbcommon openssl; do
            pkg-config --exists "$PKG" || MISSING="$MISSING $PKG"
        done
    else
        MISSING=" pkg-config"
    fi
    [ -f /usr/include/xdo.h ] || [ -f /usr/local/include/xdo.h ] || MISSING="$MISSING libxdo"
    if [ -n "$MISSING" ]; then
        fail "Missing build dependencies:$MISSING

On Debian/Ubuntu install them with:

  sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \\
    librsvg2-dev libasound2-dev libxdo-dev libxtst-dev libxkbcommon-dev libssl-dev pkg-config patchelf

(Other distributions: the equivalent WebKitGTK 4.1, GTK 3, appindicator,
librsvg, ALSA, xdo, XTest, xkbcommon and OpenSSL development packages.)"
    fi

    if [ "${XDG_SESSION_TYPE:-}" = "wayland" ] || [ -n "${WAYLAND_DISPLAY:-}" ]; then
        printf '\033[33mNote:\033[0m this looks like a Wayland session. voice needs an X11 session\n'
        printf '      to see the talk key in other apps; it will install but not hear the key here.\n'
    fi
fi

if ! cargo tauri --version >/dev/null 2>&1; then
    bold "Installing tauri-cli (one-time, a few minutes)..."
    cargo install tauri-cli --version "^2" --locked
fi

# ---------------------------------------------------------------------------
# Fetch and build

WORKDIR=$(mktemp -d "${TMPDIR:-/tmp}/voice-install.XXXXXX")
trap 'rm -rf "$WORKDIR"' EXIT

bold "Downloading voice..."
git clone --quiet --depth 1 "$REPO_URL" "$WORKDIR/voice"

# Fresh clones rebuild every dependency; a warm cargo cache still takes a
# couple of minutes for Tauri + WebKit bindings.
bold "Building (this takes a few minutes the first time)..."
CLI_SRC=""
if [ "$OS" = "Darwin" ]; then
    (cd "$WORKDIR/voice" && ./build.sh)

    bold "Installing to $DEST..."
    # Quit a running copy so the bundle can be replaced cleanly.
    osascript -e 'tell application "Voice" to quit' >/dev/null 2>&1 || true
    pkill -x Voice 2>/dev/null || true
    rm -rf "$DEST"
    cp -R "$WORKDIR/voice/$APP" "$DEST"
    CLI_SRC="$DEST/Contents/MacOS/voicectl"

    open "$DEST"
else
    (cd "$WORKDIR/voice/crates/voice-app" && cargo tauri build)
    # The Linux bundles do not carry voicectl; it is a separate package.
    (cd "$WORKDIR/voice" && cargo build --release -p voicectl)
    BUNDLE="$WORKDIR/voice/target/release/bundle"

    pkill -x voice 2>/dev/null || true

    DEB=$(find "$BUNDLE/deb" -maxdepth 1 -name '*.deb' -print -quit 2>/dev/null || true)
    APPIMAGE=$(find "$BUNDLE/appimage" -maxdepth 1 -name '*.AppImage' -print -quit 2>/dev/null || true)
    if have dpkg && [ -n "$DEB" ]; then
        bold "Installing $(basename "$DEB") (sudo may prompt)..."
        sudo dpkg -i "$DEB"
        LAUNCH="voice"
    elif [ -n "$APPIMAGE" ]; then
        bold "Installing AppImage to ~/.local/bin/voice..."
        mkdir -p "$HOME/.local/bin"
        install -m 755 "$APPIMAGE" "$HOME/.local/bin/voice"
        LAUNCH="$HOME/.local/bin/voice"
    else
        fail "cargo tauri build produced neither a .deb nor an AppImage under $BUNDLE."
    fi

    # voicectl is copied (not linked) because the build tree is deleted on exit.
    mkdir -p "$HOME/.local/bin"
    install -m 755 "$WORKDIR/voice/target/release/voicectl" "$HOME/.local/bin/voicectl"
    CLI_SRC="$HOME/.local/bin/voicectl"

    # Detach from this terminal so closing it does not take the app down.
    nohup "$LAUNCH" >/dev/null 2>&1 &
fi

# ---------------------------------------------------------------------------
# voicectl on PATH
#
# macOS: a symlink (not a copy) keeps it in lockstep with the installed app.
# Prefer a bin dir that is already on PATH and writable without sudo.
# Linux: ~/.local/bin already holds the binary; just report whether PATH has it.
CLI_LINK=""
if [ "$OS" = "Darwin" ]; then
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
else
    case ":$PATH:" in
        *":$HOME/.local/bin:"*) CLI_LINK="$CLI_SRC" ;;
    esac
fi

bold "Done! voice is running in your tray / menu bar (waveform icon)."
if [ "$OS" = "Darwin" ]; then
    cat <<'EOF'

Two one-time permission grants and you're dictating:

  1. Microphone - approve the popup that just appeared.
  2. Accessibility - System Settings > Privacy & Security > Accessibility,
     toggle Voice on. (Needed to detect the talk key and type text for you.)

On first launch the app downloads a small speech model (~142 MB) in the
background - the tray icon shows progress.

Then click into any text field, hold Right Option, speak, release.
Your words appear at the cursor. Esc cancels a recording.
EOF
else
    cat <<'EOF'

On first launch the app downloads a small speech model (~142 MB) in the
background - the tray icon shows progress.

Then click into any text field, hold Right Alt, speak, release.
Your words appear at the cursor. Esc cancels a recording. The talk key
needs an X11 session (Wayland is not supported yet).
EOF
fi

if [ -n "$CLI_LINK" ]; then
    printf '\nThe voicectl command is installed at %s\n' "$CLI_LINK"
    printf 'Try:  voicectl snippets add brb "be right back"\n'
elif [ "$OS" = "Darwin" ]; then
    printf '\nTo use the voicectl command (edit snippets from a script), link it onto PATH:\n'
    printf '  sudo ln -sfn "%s" /usr/local/bin/voicectl\n' "$CLI_SRC"
else
    printf '\nvoicectl was installed to %s; add ~/.local/bin to your PATH to use it.\n' "$CLI_SRC"
fi
