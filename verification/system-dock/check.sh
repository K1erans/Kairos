#!/bin/bash
# Checks the Dock-takeover models.
#   check.sh [--mutants]            the Mac model (SystemDock.tla), and with --mutants that each property can fail
#   check.sh --linux [--mutants]    the Linux model (CinnamonPanel.tla) against the Rust code
#   check.sh --all [--mutants]      both
#   check.sh --confirm [--linux|--all]   record the covered files as read side by side with the model
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
JAR="$HERE/../strict-mode/.tools/tla2tools.jar"
[ -f "$JAR" ] || { echo "Run verification/strict-mode/check.sh once first: it downloads TLC."; exit 3; }
ROOT="$(cd "$HERE/../.." && pwd)"

MUTANTS=0; CONFIRM=0; MAC=1; LINUX=0
for arg in "$@"; do
  case "$arg" in
    --mutants) MUTANTS=1 ;;
    --confirm) CONFIRM=1 ;;
    --mac) MAC=1; LINUX=0 ;;
    --linux) MAC=0; LINUX=1 ;;
    --all) MAC=1; LINUX=1 ;;
    *) echo "Unknown option $arg. Use --mutants, --confirm, --mac, --linux or --all."; exit 2 ;;
  esac
done

MAC_COVERED=(apps/desktop/Sources/KairosCore/SystemDock.swift apps/desktop/Sources/Kairos/KairosApp.swift)
LINUX_COVERED=(apps/linux/crates/kairos-core/src/panel.rs apps/linux/crates/kairos/src/main.rs)
hashes() { (cd "$ROOT" && shasum -a 256 "$@"); }

if [ "$CONFIRM" = 1 ]; then
  if [ "$MAC" = 1 ]; then hashes "${MAC_COVERED[@]}" > "$HERE/synced.sha256"; echo "Recorded ${#MAC_COVERED[@]} covered files as in sync."; fi
  if [ "$LINUX" = 1 ]; then hashes "${LINUX_COVERED[@]}" > "$HERE/synced-linux.sha256"; echo "Recorded ${#LINUX_COVERED[@]} covered Linux files as in sync."; fi
  exit 0
fi

note() { # sumfile what files...
  local sums="$1" what="$2"; shift 2
  if [ -f "$HERE/$sums" ] && ! diff -q <(hashes "$@") "$HERE/$sums" >/dev/null; then
    echo "NOTE  covered files changed since the model was last confirmed:"
    diff <(hashes "$@") "$HERE/$sums" | grep '^<' | awk '{print "      " $3}'
    echo "      $what"
  fi
}

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
cp "$HERE/SystemDock.tla" "$HERE/CinnamonPanel.tla" "$WORK/"

run() { # module "props" label "CONSTANT = value ..."
  local module="$1" props="$2" label="$3" bad=0
  for inv in $props; do
    { printf 'SPECIFICATION Spec\nCONSTANTS\n'
      for c in $4; do printf '  %s\n' "${c/=/ = }"; done
      printf 'INVARIANT %s\n' "$inv"; } > "$WORK/$module-$inv.cfg"
    out=$(cd "$WORK" && java -cp "$JAR" tlc2.TLC -config "$module-$inv.cfg" "$module.tla" 2>&1)
    if echo "$out" | grep -q "No error has been found"; then echo "  ok    $label $inv"
    else echo "  FAIL  $label $inv"; bad=1
      echo "$out" | awk '/^State [0-9]+:/{s=$0} /^\/\\ /{l=l" "$2$3$4} /^$/{if(s!=""){print "        "s l; s="";l=""}}'
    fi
  done
  return $bad
}

status=0
if [ "$MAC" = 1 ]; then
  [ "$LINUX" = 1 ] && echo "== Mac: SystemDock.tla"
  note synced.sha256 "Read replace(), restore(), the watchdog and the Dock sinks against SystemDock.tla, then run check.sh --confirm." "${MAC_COVERED[@]}"
  PROPS="NeverSavesOwn WayBack WatchdogCovers BackWhenNotReplacing"
  echo "The code as it is:"; run SystemDock "$PROPS" code "RecoversFromScript=TRUE RefusesOwnValues=TRUE KeepsRecoveryOnFail=TRUE" || status=1
  if [ "$MUTANTS" = 1 ]; then
    echo "Without running a leftover script first:"; run SystemDock "$PROPS" no-recover "RecoversFromScript=FALSE RefusesOwnValues=TRUE KeepsRecoveryOnFail=TRUE"
    echo "Without refusing Kairos's own values:"; run SystemDock "$PROPS" no-refuse "RecoversFromScript=FALSE RefusesOwnValues=FALSE KeepsRecoveryOnFail=TRUE"
    echo "Without keeping recovery data on restore failure:"; run SystemDock "$PROPS" drop-on-fail "RecoversFromScript=TRUE RefusesOwnValues=TRUE KeepsRecoveryOnFail=FALSE"
  fi
fi

if [ "$LINUX" = 1 ]; then
  [ "$MAC" = 1 ] && echo "== Linux: CinnamonPanel.tla"
  note synced-linux.sha256 "Read panel.rs (replace, restore, the script, the timer) and sync_panel(), finish() and --restore-panel in main.rs against CinnamonPanel.tla, then run check.sh --confirm --linux." "${LINUX_COVERED[@]}"
  PROPS="NeverSavesOwn WayBack WatchdogCovers BackWhenNotReplacing LeavesOwnersPanel"
  # The Rust code has all four fixes (panel.rs: recover(), is_own_setting(), the script's rm "$0",
  # panel.lock). Set a switch to FALSE here if the code loses one.
  CODE="RecoversFromScript=TRUE RefusesOwnValues=TRUE ScriptRemovesItself=TRUE ScriptLocked=TRUE"
  echo "The code as it is:"; run CinnamonPanel "$PROPS" code "$CODE" || status=1
  if [ "$MUTANTS" = 1 ]; then
    # Each fix switched off on its own, to show each is needed.
    FIXED="$CODE"
    echo "Before the fixes (the code until 2026-11-21):"; run CinnamonPanel "$PROPS" unfixed "RecoversFromScript=FALSE RefusesOwnValues=FALSE ScriptRemovesItself=FALSE ScriptLocked=FALSE"
    echo "Without running a leftover script first:"; run CinnamonPanel "$PROPS" no-recover "${FIXED/RecoversFromScript=TRUE/RecoversFromScript=FALSE}"
    echo "Without refusing Kairos's own values (a backstop: holds unless restore-panel.sh is deleted, which is not modelled):"
    run CinnamonPanel "$PROPS" no-refuse "${FIXED/RefusesOwnValues=TRUE/RefusesOwnValues=FALSE}"
    echo "With a script that stays after it runs:"; run CinnamonPanel "$PROPS" script-stays "${FIXED/ScriptRemovesItself=TRUE/ScriptRemovesItself=FALSE}"
    echo "Without the lock between the script and Kairos:"; run CinnamonPanel "$PROPS" no-lock "${FIXED/ScriptLocked=TRUE/ScriptLocked=FALSE}"
  fi
fi
exit $status
