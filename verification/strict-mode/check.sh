#!/bin/bash
# Checks the strict-mode model against the code it describes.
#   check.sh            anchors, then every property; exit 0 when results match expected.tsv
#   check.sh --mutants  also prove each property can fail on a broken variant
#   check.sh --confirm  record the covered files as confirmed in sync (run only after reading them)
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
JAR="$HERE/.tools/tla2tools.jar"
JAR_URL="https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar"
JAR_SHA="936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
COVERED=(apps/desktop/Sources/KairosCore/Rules.swift apps/desktop/Sources/KairosCore/Tempo.swift
         apps/desktop/Sources/KairosCore/Models.swift apps/desktop/Sources/KairosCore/History.swift
         apps/desktop/Sources/Kairos/AppModel.swift apps/desktop/Sources/Kairos/KairosApp.swift
         apps/desktop/Sources/Kairos/KairosBar.swift)

hashes() { (cd "$ROOT" && shasum -a 256 "${COVERED[@]}"); }

if [ "${1:-}" = "--confirm" ]; then
  hashes > "$HERE/synced.sha256"
  echo "Recorded ${#COVERED[@]} covered files as in sync."
  exit 0
fi

command -v java >/dev/null || { echo "java is needed (brew install openjdk)"; exit 3; }
if [ ! -f "$JAR" ]; then
  mkdir -p "$HERE/.tools"
  curl -sL -o "$JAR" "$JAR_URL" || { echo "could not download TLC"; exit 3; }
fi
[ "$(shasum -a 256 "$JAR" | cut -d' ' -f1)" = "$JAR_SHA" ] || { echo "tla2tools.jar has the wrong checksum; delete $JAR"; exit 3; }

# 1. Anchors: each line of code the model mirrors must still be there, the expected number of times.
drift=0
while IFS=$'\t' read -r file count text mirrors; do
  case "$file" in ''|'#'*) continue;; esac
  found=$(grep -cF -- "$text" "$ROOT/$file" 2>/dev/null || true)
  if [ "${found:-0}" != "$count" ]; then
    echo "OUT OF SYNC  $file: expected $count, found ${found:-0}: $text"
    echo "             model element to re-read: $mirrors"
    drift=1
  fi
done < "$HERE/anchors.tsv"
if [ $drift = 1 ]; then
  echo "The code the model describes has changed. Update StrictMode.tla and anchors.tsv to match it, then rerun."
  exit 2
fi
if [ -f "$HERE/synced.sha256" ] && ! diff -q <(hashes) "$HERE/synced.sha256" >/dev/null; then
  echo "NOTE  covered files changed since the model was last confirmed (anchors still match):"
  diff <(hashes) "$HERE/synced.sha256" | grep '^<' | awk '{print "      " $3}'
  echo "      Read the changes for new ways to change mode, timer or overrides, then run check.sh --confirm."
fi

# 2. Properties. run <label> <invariant> <constant overrides...> prints PASS or FAIL.
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cp "$HERE/StrictMode.tla" "$WORK/"
run() {
  local label="$1" inv="$2"; shift 2
  local refuses=TRUE checks=TRUE syncs=TRUE picker=TRUE resets=TRUE locks=TRUE quit=TRUE resumes=TRUE rules=TRUE
  for kv in "$@"; do
    case "$kv" in
      LedgerRefusesStrict=*) refuses="${kv#*=}";; LedgerChecksMax=*) checks="${kv#*=}";;
      SyncsModeFromTimer=*) syncs="${kv#*=}";; PickerLockedByTimer=*) picker="${kv#*=}";;
      ResetsOnEnterStrict=*) resets="${kv#*=}";; LocksStrictControls=*) locks="${kv#*=}";;
      QuitLocked=*) quit="${kv#*=}";; ResumesStrict=*) resumes="${kv#*=}";;
      LocksRules=*) rules="${kv#*=}";;
    esac
  done
  cat > "$WORK/$label.cfg" <<CFG
SPECIFICATION Spec
CONSTANTS
  Max = 3
  LedgerRefusesStrict = $refuses
  LedgerChecksMax = $checks
  SyncsModeFromTimer = $syncs
  PickerLockedByTimer = $picker
  ResetsOnEnterStrict = $resets
  LocksStrictControls = $locks
  QuitLocked = $quit
  ResumesStrict = $resumes
  LocksRules = $rules
CONSTRAINT Bound
INVARIANT $inv
CFG
  (cd "$WORK" && java -XX:+UseParallelGC -cp "$JAR" tlc2.TLC -config "$label.cfg" -metadir "$WORK/states-$label" -workers 1 StrictMode.tla > "$label.out" 2>&1)
  if grep -q "No error has been found" "$WORK/$label.out"; then echo PASS
  elif grep -q "Invariant $inv is violated" "$WORK/$label.out"; then echo FAIL
  else echo ERROR; fi
}

trace() { # the steps of a counterexample, one per line
  awk '/^State [0-9]+:/{getline; s=""} /^\/\\ act = /{a=$4} /^\/\\ mode = /{m=$4} /^\/\\ phase = /{p=$4} /^\/\\ grant = /{g=$4} /^$/{if(a!=""){printf "        %-14s mode=%-9s timer=%-8s grant=%s\n",a,m,p,g; a=""}}' "$1" | tr -d '"'
}

status=0
echo "Properties on the code as it is (Max = 3):"
while IFS=$'\t' read -r inv expected note; do
  case "$inv" in ''|'#'*) continue;; esac
  got=$(run "code-$inv" "$inv")
  if [ "$got" = "$expected" ]; then
    if [ "$got" = FAIL ]; then echo "  KNOWN FINDING  $inv: $note"; trace "$WORK/code-$inv.out"
    else echo "  ok             $inv"; fi
  else
    echo "  CHANGED        $inv: expected $expected, got $got"
    [ "$got" = FAIL ] && trace "$WORK/code-$inv.out"
    [ "$got" = ERROR ] && tail -15 "$WORK/code-$inv.out"
    status=1
  fi
done < "$HERE/expected.tsv"

if [ "${1:-}" = "--mutants" ]; then
  echo "Each property must fail on a broken variant:"
  expect() { local want="$1" label="$2" inv="$3"; shift 3; local got; got=$(run "$label" "$inv" "$@")
    if [ "$got" = "$want" ]; then echo "  ok             $label -> $got"; else echo "  WRONG          $label: wanted $want, got $got"; status=1; fi; }
  expect FAIL mutant-no-max-check        UsedBounded      LedgerChecksMax=FALSE
  expect FAIL mutant-no-sync             ModeMatchesTimer SyncsModeFromTimer=FALSE
  expect FAIL mutant-ledger-allows-strict StrictNoGrant   LedgerRefusesStrict=FALSE
  expect FAIL mutant-no-reset-on-strict  StrictNoGrant    ResetsOnEnterStrict=FALSE
  expect FAIL mutant-picker-unlocked     StrictNoEscape   PickerLockedByTimer=FALSE
  expect FAIL mutant-skip-stop-unlocked  StrictNoEscape   LocksStrictControls=FALSE
  expect FAIL mutant-quit-unlocked       QuitRefusedInStrict QuitLocked=FALSE
  expect FAIL mutant-no-resume           StrictNoEscape   ResumesStrict=FALSE
  expect FAIL mutant-rules-unlocked      StrictRulesHold  LocksRules=FALSE
fi

[ $status = 0 ] && echo "Result: matches expected.tsv." || echo "Result: DIFFERS from expected.tsv."
exit $status
