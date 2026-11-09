#!/bin/bash
# Builds build/Kairos.app from the Swift package. Usage: scripts/build-app.sh [--install]
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$(cd ../.. && pwd)"
APP="build/Kairos.app"

swift build -c release
BIN="$(swift build -c release --show-bin-path)"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN/Kairos" "$BIN/KairosHost" "$APP/Contents/MacOS/"
mv "$APP/Contents/MacOS/KairosHost" "$APP/Contents/MacOS/kairos-host"
cp Resources/Info.plist "$APP/Contents/Info.plist"

# Icon
ICONSET="build/Kairos.iconset"
rm -rf "$ICONSET"
swift scripts/make-icon.swift app "$ICONSET"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Kairos.icns"
rm -rf "$ICONSET"

# Ship the extensions inside the app so Settings can reveal them in Finder.
for ext in shield read; do
  if [ -f "$ROOT/extensions/$ext/manifest.json" ]; then
    mkdir -p "$APP/Contents/Resources/extensions"
    rsync -a --exclude test --exclude node_modules --exclude '.*' "$ROOT/extensions/$ext" "$APP/Contents/Resources/extensions/"
  fi
done

# Developer ID keeps the same identity across rebuilds, so Keychain and Accessibility grants stick.
IDENTITY="$(security find-identity -v -p codesigning | awk -F'"' '/Developer ID Application/ {print $2; exit}')"
if [ -n "$IDENTITY" ]; then
  codesign --force --options runtime --timestamp=none --sign "$IDENTITY" "$APP/Contents/MacOS/kairos-host"
  codesign --force --options runtime --timestamp=none --sign "$IDENTITY" "$APP"
else
  codesign --force --sign - "$APP/Contents/MacOS/kairos-host"
  codesign --force --sign - "$APP"
fi
codesign --verify --strict "$APP"
echo "Built $APP"

if [ "${1:-}" = "--install" ]; then
  DEST="/Applications/Kairos.app"
  # Copy beside it first, so a failed copy never leaves half an app; build/Kairos.app stays.
  rm -rf "$DEST.new"
  ditto "$APP" "$DEST.new"
  rm -rf "$DEST"
  mv "$DEST.new" "$DEST"
  echo "Installed $DEST"
  # The browsers start the native host named in their manifests; point them at the installed copy.
  "$DEST/Contents/MacOS/Kairos" --install-bridge
  cat <<'NEXT'

To switch to the installed copy:
  1. Quit the Kairos that is running now: its menu bar icon > Quit Kairos, or Cmd-Q in its window.
     During a strict focus session Quit stays off until the session ends; switch after that.
  2. Open /Applications/Kairos.app. Your data, settings and keys carry over.
  3. If "Open Kairos at login" is on, turn it off and on again in Settings so it opens this copy.
  4. If Settings shows Accessibility as not allowed, allow /Applications/Kairos.app again in
     System Settings > Privacy & Security > Accessibility (macOS asks again for an ad hoc signed copy).
  Kairos Shield and Kairos Read reconnect on their own; reload them in chrome://extensions if not.
NEXT
fi
