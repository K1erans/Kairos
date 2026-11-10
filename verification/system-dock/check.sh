#!/bin/bash
# Checks the Dock-takeover model.
#   check.sh [--mutants]   every property, and with --mutants that each can fail
#   check.sh --confirm     record the covered files as read side by side with the model
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
JAR="$HERE/../strict-mode/.tools/tla2tools.jar"
[ -f "$JAR" ] || { echo "Run verification/strict-mode/check.sh once first: it downloads TLC."; exit 3; }
ROOT="$(cd "$HERE/../.." && pwd)"
COVERED=(apps/desktop/Sources/KairosCore/SystemDock.swift apps/desktop/Sources/Kairos/KairosApp.swift)
hashes() { (cd "$ROOT" && shasum -a 256 "${COVERED[@]}"); }
if [ "${1:-}" = "--confirm" ]; then hashes > "$HERE/synced.sha256"; echo "Recorded ${#COVERED[@]} covered files as in sync."; exit 0; fi
if [ -f "$HERE/synced.sha256" ] && ! diff -q <(hashes) "$HERE/synced.sha256" >/dev/null; then
  echo "NOTE  covered files changed since the model was last confirmed:"
  diff <(hashes) "$HERE/synced.sha256" | grep '^<' | awk '{print "      " $3}'
  echo "      Read replace(), restore(), the watchdog and the Dock sinks against SystemDock.tla, then run check.sh --confirm."
fi
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
cp "$HERE/SystemDock.tla" "$WORK/"
PROPS="NeverSavesOwn WayBack WatchdogCovers BackWhenNotReplacing"
run() { # label recovers refuses
  local bad=0
  for inv in $PROPS; do
    printf 'SPECIFICATION Spec\nCONSTANTS\n  RecoversFromScript = %s\n  RefusesOwnValues = %s\nINVARIANT %s\n' "$2" "$3" "$inv" > "$WORK/$inv.cfg"
    out=$(cd "$WORK" && java -cp "$JAR" tlc2.TLC -config "$inv.cfg" SystemDock.tla 2>&1)
    if echo "$out" | grep -q "No error has been found"; then echo "  ok    $1 $inv"
    else echo "  FAIL  $1 $inv"; bad=1
      echo "$out" | awk '/^State [0-9]+:/{s=$0} /^\/\\ /{l=l" "$2$3$4} /^$/{if(s!=""){print "        "s l; s="";l=""}}'
    fi
  done
  return $bad
}
echo "The code as it is:"; run code TRUE TRUE; status=$?
if [ "${1:-}" = "--mutants" ]; then
  echo "Without running a leftover script first:"; run no-recover FALSE TRUE
  echo "Without refusing Kairos's own values:"; run no-refuse FALSE FALSE
fi
exit $status
