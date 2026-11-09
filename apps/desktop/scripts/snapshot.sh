#!/bin/bash
# Renders every Kairos view to PNG offscreen, against a scratch store, so the UI can be
# reviewed without a screen. Usage: scripts/snapshot.sh [dir] [--light]
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${1:-build/snapshots}"
swift build
KAIROS_HOME="$(mktemp -d)" "$(swift build --show-bin-path)/Kairos" --snapshot "$OUT" "${@:2}"
