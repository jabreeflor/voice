#!/bin/bash
# Build Voice.app — local, free voice dictation.
set -euo pipefail
cd "$(dirname "$0")"

APP="Voice.app"

echo "Compiling…"
swift build -c release

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp Info.plist "$APP/Contents/Info.plist"
cp AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp "$(swift build -c release --show-bin-path)/Voice" "$APP/Contents/MacOS/Voice"

# Sign with a real identity when one exists — a stable signature means macOS
# keeps the Accessibility/Microphone grants across rebuilds. Ad-hoc fallback.
IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null \
    | sed -n 's/^ *[0-9]*) [0-9A-F]* "\(.*\)"$/\1/p' | head -1)
if [ -n "$IDENTITY" ]; then
    echo "Signing with: $IDENTITY"
    codesign --force --sign "$IDENTITY" "$APP"
else
    echo "Signing ad-hoc (permissions will need re-granting after each rebuild)"
    codesign --force --sign - "$APP"
fi

echo "Built $APP"
echo "Run with:  open $APP"
