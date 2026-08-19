#!/bin/bash
# Build Voice.app — local, free voice dictation.
set -euo pipefail
cd "$(dirname "$0")"

APP="Voice.app"

echo "Compiling…"
swift build -c release

BIN="$(swift build -c release --show-bin-path)"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp Info.plist "$APP/Contents/Info.plist"
cp AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp "$BIN/Voice" "$APP/Contents/MacOS/Voice"
cp "$BIN/voice-cli" "$APP/Contents/MacOS/voice"

# Sign with a real identity when one exists — a stable signature means macOS
# keeps the Accessibility/Microphone grants across rebuilds. Ad-hoc fallback.
IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null \
    | sed -n 's/^ *[0-9]*) [0-9A-F]* "\(.*\)"$/\1/p' | head -1)
sign() {
    if [ -n "$IDENTITY" ]; then
        codesign --force --sign "$IDENTITY" "$1"
    else
        codesign --force --sign - "$1"
    fi
}
if [ -n "$IDENTITY" ]; then
    echo "Signing with: $IDENTITY"
else
    echo "Signing ad-hoc (permissions will need re-granting after each rebuild)"
fi
sign "$APP/Contents/MacOS/voice"
sign "$APP"

echo "Built $APP"
echo "CLI:  $APP/Contents/MacOS/voice"
echo "Run with:  open $APP"
