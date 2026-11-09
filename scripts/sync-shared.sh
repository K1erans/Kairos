#!/bin/sh
# Copies the shared modules into both extensions (the copies are committed).
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
for ext in shield read; do
  for f in bridge.js match.js; do
    cp "$ROOT/packages/shared/$f" "$ROOT/extensions/$ext/lib/$f"
  done
done
echo "synced shared modules"
