#!/bin/bash
# Checks the Dock-takeover model.  check.sh [--mutants]
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
JAR="$HERE/../strict-mode/.tools/tla2tools.jar"
[ -f "$JAR" ] || { echo "Run verification/strict-mode/check.sh once first: it downloads TLC."; exit 3; }
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
