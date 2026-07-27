#!/usr/bin/env bash
#
# Tier-4 end-to-end smoke test for Voice — MANUAL ONLY, never run in CI.
#
# Drives the real app the way a person does: holds the Right-Option hotkey via
# synthesized CGEvents, speaks a phrase out loud through `say`, releases, and
# checks that the transcription landed in a TextEdit document.
#
# This needs a real logged-in GUI session with TCC permissions already granted,
# which is exactly what a CI runner does not have. See scripts/README.md.
#
# Not `set -e`: a failed assertion should still reach the cleanup trap and print
# a verdict rather than abort halfway through with the TextEdit doc left open.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$REPO_ROOT/Voice.app"
PORT=8178

# Short, phonetically distinct, and unlikely to appear in a stray transcription.
PHRASE="the quick brown fox jumps over the lazy dog"
KEYWORD="brown fox"

AUTO_YES=0
[ "${1:-}" = "-y" ] && AUTO_YES=1

step()  { printf '\n\033[1;34m==>\033[0m \033[1m%s\033[0m\n' "$*"; }
info()  { printf '    %s\n' "$*"; }
warn()  { printf '\033[1;33m[warn]\033[0m %s\n' "$*"; }
fail()  { printf '\033[1;31m[fail]\033[0m %s\n' "$*"; }

DOC_NAME=""

cleanup() {
    # Only close the document this script created. Matching on the name we
    # captured at creation time keeps an unrelated TextEdit window the user
    # happens to have open from being discarded.
    if [ -n "$DOC_NAME" ]; then
        step "Cleaning up"
        osascript <<OSA >/dev/null 2>&1
tell application "TextEdit"
    if exists document "$DOC_NAME" then close document "$DOC_NAME" saving no
end tell
OSA
        info "Closed scratch document '$DOC_NAME' without saving."
    fi
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
step "Preflight"

PREFLIGHT_OK=1

if [ -x "$APP/Contents/MacOS/Voice" ]; then
    info "Voice.app: $APP"
else
    fail "Voice.app not built. Run ./build.sh first."
    PREFLIGHT_OK=0
fi

WHISPER_SERVER=""
for candidate in /opt/homebrew/bin/whisper-server \
                 /usr/local/bin/whisper-server \
                 /opt/homebrew/opt/whisper-cpp/bin/whisper-server; do
    [ -x "$candidate" ] && { WHISPER_SERVER="$candidate"; break; }
done
if [ -n "$WHISPER_SERVER" ]; then
    info "whisper-server: $WHISPER_SERVER"
else
    fail "whisper-server not found. Run: brew install whisper-cpp"
    PREFLIGHT_OK=0
fi

MODEL=""
for dir in "$HOME/thevoice/models" "$HOME/.thevoice/models"; do
    [ -d "$dir" ] || continue
    found=$(find "$dir" -maxdepth 1 -name 'ggml*.bin' -print -quit 2>/dev/null)
    [ -n "$found" ] && { MODEL="$found"; break; }
done
if [ -n "$MODEL" ]; then
    info "model: $MODEL"
else
    fail "No ggml model found in ~/thevoice/models or ~/.thevoice/models."
    info "Launch Voice.app once and let it auto-download, or fetch base.en manually."
    PREFLIGHT_OK=0
fi

[ "$PREFLIGHT_OK" -eq 1 ] || { fail "Preflight failed — nothing was run."; exit 1; }

# ---------------------------------------------------------------------------
step "Read this before continuing"
cat <<'NOTE'
    This script synthesizes real keystrokes and speaks out loud. While it runs:

      * Do not type or click. Synthesized events go to whatever is focused.
      * Voice.app needs Accessibility + Microphone already granted in
        System Settings > Privacy & Security. The script cannot grant them.
      * The terminal running this script also needs Accessibility (to post
        events) and Automation > TextEdit (to script it). macOS will prompt on
        first run; approving mid-run will make this attempt fail — rerun after.
      * Your Mac will say a test phrase aloud and the microphone must hear it.
        A quiet room and normal speaker volume are enough. Headphones are not,
        since the mic then hears nothing. See scripts/README.md for the
        BlackHole loopback setup if you want this to be deterministic.
NOTE

if [ "$AUTO_YES" -eq 0 ]; then
    printf '\n    Press Enter to run, Ctrl-C to abort: '
    read -r _
fi

# ---------------------------------------------------------------------------
step "Starting Voice.app"

if pgrep -f "Voice.app/Contents/MacOS/Voice" >/dev/null 2>&1; then
    info "Already running — leaving it alone."
else
    open "$APP"
    info "Launched. Giving it a moment to spawn whisper-server…"
fi

# ---------------------------------------------------------------------------
step "Waiting for the whisper engine on port $PORT"

ENGINE_UP=0
for i in $(seq 1 90); do
    if curl -s -o /dev/null --max-time 2 "http://127.0.0.1:$PORT/"; then
        ENGINE_UP=1
        info "Engine accepting connections after ${i}s."
        break
    fi
    sleep 1
done

if [ "$ENGINE_UP" -eq 0 ]; then
    fail "Engine never came up on port $PORT after 90s."
    info "Check the Voice menu bar item for its status text."
    exit 1
fi

# The model is loaded but the first inference compiles Metal kernels. The app
# warms up on its own; this just avoids racing that warm-up.
info "Letting the engine warm up…"
sleep 3

# ---------------------------------------------------------------------------
step "Opening a scratch TextEdit document"

DOC_NAME=$(osascript <<'OSA'
tell application "TextEdit"
    activate
    set newDoc to make new document
    return name of newDoc
end tell
OSA
)

if [ -z "$DOC_NAME" ]; then
    fail "Could not create a TextEdit document. Automation permission denied?"
    exit 1
fi
info "Document: '$DOC_NAME' (focused)"
sleep 1

# ---------------------------------------------------------------------------
step "Dictating: \"$PHRASE\""

# Right-Option is keycode 61. The app's event tap listens for .flagsChanged
# with .maskAlternate set (press) and cleared (release), so a plain keyDown is
# not enough — the event type has to be rewritten to flagsChanged.
#
# `say` runs synchronously between press and release, which is what makes the
# hold last as long as the speech. The recording is only as good as what the
# mic picks up off the speakers; that is the known soft spot of this test.
SMOKE_PHRASE="$PHRASE" swift - <<'EOF'
import Foundation
import CoreGraphics

let phrase = ProcessInfo.processInfo.environment["SMOKE_PHRASE"] ?? "hello"
let rightOption: CGKeyCode = 61

func postFlags(down: Bool) {
    let src = CGEventSource(stateID: .hidSystemState)
    guard let e = CGEvent(keyboardEventSource: src,
                          virtualKey: rightOption,
                          keyDown: down) else {
        FileHandle.standardError.write(Data("could not create CGEvent\n".utf8))
        exit(1)
    }
    e.type = .flagsChanged
    e.flags = down ? .maskAlternate : []
    e.post(tap: .cghidEventTap)
}

func say(_ text: String) {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/usr/bin/say")
    p.arguments = [text]
    try? p.run()
    p.waitUntilExit()
}

print("    hotkey down")
postFlags(down: true)

// Let the recorder actually start before any audio exists to capture.
Thread.sleep(forTimeInterval: 0.7)

print("    speaking…")
say(phrase)

// Trailing pad so the tail of the phrase is inside the recording window.
Thread.sleep(forTimeInterval: 0.8)

print("    hotkey up")
postFlags(down: false)
EOF

SPEAK_RC=$?
if [ "$SPEAK_RC" -ne 0 ]; then
    fail "Keystroke synthesis failed (exit $SPEAK_RC)."
    info "Most likely the terminal lacks Accessibility permission."
    exit 1
fi

# ---------------------------------------------------------------------------
step "Waiting for transcription to be pasted"
sleep 4

RESULT=$(osascript <<OSA
tell application "TextEdit"
    if exists document "$DOC_NAME" then
        return text of document "$DOC_NAME"
    end if
    return ""
end tell
OSA
)

info "TextEdit contains: '${RESULT}'"

# ---------------------------------------------------------------------------
step "Verdict"

if printf '%s' "$RESULT" | grep -qi "$KEYWORD"; then
    printf '\033[1;32m[PASS]\033[0m Transcription contains "%s".\n' "$KEYWORD"
    exit 0
fi

fail "Transcription did not contain \"$KEYWORD\"."
if [ -z "$RESULT" ]; then
    info "The document is empty — nothing was dictated at all. Likely causes:"
    info "  * Voice.app is missing Accessibility permission (no hotkey capture)"
    info "  * Voice.app is missing Microphone permission (silent recording)"
    info "  * The terminal is missing Accessibility permission (events dropped)"
else
    info "Audio was captured but misheard. Try raising the speaker volume,"
    info "moving somewhere quieter, or wiring up BlackHole loopback."
fi
exit 1
