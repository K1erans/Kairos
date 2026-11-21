#!/bin/bash
# Checks the strict-mode model against the code it describes.
#   check.sh            Kairos Desktop (Swift): anchors, then every property; exit 0 when results match expected.tsv
#   check.sh --linux    Kairos for Linux (Rust): the same against anchors-linux.tsv and expected-linux.tsv
#   check.sh --all      both, Mac first
#   check.sh --mutants  also prove each property can fail on a broken variant (combines with the above)
#   check.sh --confirm  record the covered files as confirmed in sync (run only after reading them;
#                       combines with --linux or --all)
# Exit: 0 matches, 1 a property differs, 2 out of sync, 3 setup problem. With --all, the worst of the two.
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
JAR="$HERE/.tools/tla2tools.jar"
JAR_URL="https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar"
JAR_SHA="936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
COVERED_MAC=(apps/desktop/Sources/KairosCore/Rules.swift apps/desktop/Sources/KairosCore/Tempo.swift
         apps/desktop/Sources/KairosCore/Models.swift apps/desktop/Sources/KairosCore/History.swift
         apps/desktop/Sources/Kairos/AppModel.swift apps/desktop/Sources/Kairos/KairosApp.swift
         apps/desktop/Sources/Kairos/KairosBar.swift)
COVERED_LINUX=(apps/linux/crates/kairos-core/src/rules.rs apps/linux/crates/kairos-core/src/tempo.rs
         apps/linux/crates/kairos-core/src/models.rs apps/linux/crates/kairos-core/src/history.rs
         apps/linux/crates/kairos/src/model.rs apps/linux/crates/kairos/src/main.rs
         apps/linux/crates/kairos/src/platform/tray.rs apps/linux/crates/kairos/src/ui/pages.rs
         apps/linux/crates/kairos/src/ui/window.rs apps/linux/crates/kairos/src/ui/bar.rs
         apps/linux/crates/kairos/src/ui/widgets.rs)

TARGETS=() MUTANTS=0 CONFIRM=0
for arg in "$@"; do
  case "$arg" in
    --mac) TARGETS+=(mac);; --linux) TARGETS+=(linux);; --all) TARGETS+=(mac linux);;
    --mutants) MUTANTS=1;; --confirm) CONFIRM=1;;
    *) echo "unknown option $arg (use --mac, --linux, --all, --mutants, --confirm)"; exit 3;;
  esac
done
[ ${#TARGETS[@]} = 0 ] && TARGETS=(mac)
# Headers only when the run is not the plain Mac run, so that output stays as it was.
HEADERS=0; [ "${TARGETS[*]}" != mac ] && HEADERS=1

# Per target: anchors, expected results, synced checksums, covered files, and the model's switch
# values for that code (all TRUE today on both; set one to FALSE here if a target loses a guard).
use_target() {
  case "$1" in
    mac)   NAME="Kairos Desktop (apps/desktop, Swift)"; ANCHORS="$HERE/anchors.tsv"; EXPECTED="$HERE/expected.tsv"
           SYNCED="$HERE/synced.sha256"; COVERED=("${COVERED_MAC[@]}"); SWITCHES=();;
    linux) NAME="Kairos for Linux (apps/linux, Rust)"; ANCHORS="$HERE/anchors-linux.tsv"; EXPECTED="$HERE/expected-linux.tsv"
           SYNCED="$HERE/synced-linux.sha256"; COVERED=("${COVERED_LINUX[@]}"); SWITCHES=();;
  esac
}

hashes() { (cd "$ROOT" && shasum -a 256 "${COVERED[@]}"); }

if [ $CONFIRM = 1 ]; then
  for t in "${TARGETS[@]}"; do
    use_target "$t"
    hashes > "$SYNCED"
    if [ $HEADERS = 1 ]; then echo "Recorded ${#COVERED[@]} covered files of $NAME as in sync."
    else echo "Recorded ${#COVERED[@]} covered files as in sync."; fi
  done
  exit 0
fi

command -v java >/dev/null || { echo "java is needed (brew install openjdk, or apt install openjdk-21-jre)"; exit 3; }
if [ ! -f "$JAR" ]; then
  mkdir -p "$HERE/.tools"
  curl -sL -o "$JAR" "$JAR_URL" || { echo "could not download TLC"; exit 3; }
fi
[ "$(shasum -a 256 "$JAR" | cut -d' ' -f1)" = "$JAR_SHA" ] || { echo "tla2tools.jar has the wrong checksum; delete $JAR"; exit 3; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cp "$HERE/StrictMode.tla" "$WORK/"

# run <label> <invariant> <constant overrides...> prints PASS or FAIL. The target's SWITCHES come
# first, so an override given here wins.
run() {
  local label="$1" inv="$2"; shift 2
  local refuses=TRUE checks=TRUE syncs=TRUE picker=TRUE resets=TRUE locks=TRUE quit=TRUE resumes=TRUE rules=TRUE
  for kv in ${SWITCHES[@]+"${SWITCHES[@]}"} "$@"; do
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

check_target() { # prints the result for one target and returns its exit status
  use_target "$1"
  local tag="$1" status=0
  [ $HEADERS = 1 ] && echo "== $NAME =="

  # 1. Anchors: each line of code the model mirrors must still be there, the expected number of times.
  local drift=0 file count text mirrors found
  while IFS=$'\t' read -r file count text mirrors; do
    case "$file" in ''|'#'*) continue;; esac
    found=$(grep -cF -- "$text" "$ROOT/$file" 2>/dev/null || true)
    if [ "${found:-0}" != "$count" ]; then
      echo "OUT OF SYNC  $file: expected $count, found ${found:-0}: $text"
      echo "             model element to re-read: $mirrors"
      drift=1
    fi
  done < "$ANCHORS"
  if [ $drift = 1 ]; then
    echo "The code the model describes has changed. Update StrictMode.tla and $(basename "$ANCHORS") to match it, then rerun."
    return 2
  fi
  if [ -f "$SYNCED" ] && ! diff -q <(hashes) "$SYNCED" >/dev/null; then
    echo "NOTE  covered files changed since the model was last confirmed (anchors still match):"
    diff <(hashes) "$SYNCED" | grep '^<' | awk '{print "      " $3}'
    echo "      Read the changes for new ways to change mode, timer or overrides, then run check.sh --confirm$([ "$tag" = linux ] && echo " --linux")."
  fi

  # 2. Properties.
  local inv expected note got
  echo "Properties on the code as it is (Max = 3):"
  while IFS=$'\t' read -r inv expected note; do
    case "$inv" in ''|'#'*) continue;; esac
    got=$(run "$tag-code-$inv" "$inv")
    if [ "$got" = "$expected" ]; then
      if [ "$got" = FAIL ]; then echo "  KNOWN FINDING  $inv: $note"; trace "$WORK/$tag-code-$inv.out"
      else echo "  ok             $inv"; fi
    else
      echo "  CHANGED        $inv: expected $expected, got $got"
      [ "$got" = FAIL ] && trace "$WORK/$tag-code-$inv.out"
      [ "$got" = ERROR ] && tail -15 "$WORK/$tag-code-$inv.out"
      status=1
    fi
  done < "$EXPECTED"

  if [ $MUTANTS = 1 ]; then
    echo "Each property must fail on a broken variant:"
    expect() { local want="$1" label="$2" inv="$3"; shift 3; local got; got=$(run "$tag-$label" "$inv" "$@")
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

  [ $status = 0 ] && echo "Result: matches $(basename "$EXPECTED")." || echo "Result: DIFFERS from $(basename "$EXPECTED")."
  return $status
}

overall=0
for t in "${TARGETS[@]}"; do
  check_target "$t"; s=$?
  [ $s -gt $overall ] && overall=$s
done
exit $overall
