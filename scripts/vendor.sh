#!/bin/sh
# Vendors third-party files into extensions/read/vendor (committed).
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/extensions/read/vendor"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$OUT"
cd "$TMP"
npm pack --silent @mozilla/readability@0.6.0 three@0.170.0 >/dev/null
mkdir r t
tar -xzf mozilla-readability-0.6.0.tgz -C r
tar -xzf three-0.170.0.tgz -C t
cp r/package/Readability.js "$OUT/Readability.js"
cp r/package/Readability-readerable.js "$OUT/Readability-readerable.js"
cp r/package/LICENSE.md "$OUT/Readability.LICENSE.md" 2>/dev/null || true
cp t/package/build/three.module.min.js "$OUT/three.module.js"
cp t/package/examples/jsm/controls/OrbitControls.js "$OUT/OrbitControls.js"
cp t/package/LICENSE "$OUT/three.LICENSE"
echo "vendored into $OUT"
