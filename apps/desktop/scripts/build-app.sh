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
  rm -rf "/Applications/Kairos.app"
  cp -R "$APP" /Applications/
  echo "Installed /Applications/Kairos.app"
fi
