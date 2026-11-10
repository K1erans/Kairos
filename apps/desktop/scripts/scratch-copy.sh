#!/bin/bash
# Copies a built Kairos.app for a scratch run (KAIROS_HOME) under its own bundle id, so the copy's
# window frames and other UserDefaults land in com.kairos.desktop.scratch, never in the owner's
# com.kairos.desktop (which also holds the Dock settings Kairos puts back). Signed ad hoc.
# Usage: scripts/scratch-copy.sh path/to/Kairos.app path/to/Scratch.app
set -euo pipefail
SRC="$1"
DEST="$2"
rm -rf "$DEST"
ditto "$SRC" "$DEST"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier com.kairos.desktop.scratch" "$DEST/Contents/Info.plist"
codesign --force --sign - "$DEST/Contents/MacOS/kairos-host"
codesign --force --sign - "$DEST"
codesign --verify --strict "$DEST"
echo "$DEST"
