---------------------------- MODULE SystemDock ----------------------------
(* Kairos hides the macOS Dock and promises to put back exactly what was there.
   Mirrors SystemDock and DockWatchdog (apps/desktop/Sources/KairosCore/SystemDock.swift)
   and the two sinks plus the quit paths in KairosApp.swift.

   The Dock's setting is one value: "A" is what the owner had, "B" is something else the
   owner picked later, "K" is Kairos's own hidden setting, "D" is the macOS default (keys
   deleted). `saved` is what Kairos remembers in its preferences, `script` what the restore
   script on disk would put back. *)
EXTENDS Naturals

CONSTANTS
  RecoversFromScript,  \* replace()/restore() with nothing saved run a leftover script first
  RefusesOwnValues,    \* replace() never records Kairos's own hidden setting as the owner's
  KeepsRecoveryOnFail  \* restore()/recover()/script keep saved prefs and script when defaults fails

VARIABLES running, replaces, visible, dock, saved, script, restoring, watchdog, armed

vars == <<running, replaces, visible, dock, saved, script, restoring, watchdog, armed>>
Values == {"A", "B", "K", "D"}
None == "none"

Init ==
  /\ running = FALSE /\ replaces = FALSE /\ visible = TRUE
  /\ dock = "A" /\ saved = None /\ script = None /\ restoring = FALSE
  /\ watchdog = FALSE /\ armed = FALSE

hides == replaces /\ visible

\* Value saved by replace() given what is currently read from com.apple.dock.
Kept(base) == IF RefusesOwnValues /\ base = "K" THEN "D" ELSE base

\* SystemDock.replace().
DoReplace ==
  IF saved # None THEN
    \* Already hidden or mid-restore: preserve owner's saved values, rewrite full script, re-hide.
    /\ dock' = "K" /\ saved' = saved /\ script' = saved /\ restoring' = FALSE
  ELSE IF RecoversFromScript /\ script # None THEN
    \/ \* recover() succeeds: restore from script, then record owner's values and hide.
       /\ dock' = "K" /\ saved' = Kept(script) /\ script' = Kept(script) /\ restoring' = FALSE
    \/ \* recover() fails: fixed code keeps script intact and returns early; buggy code deletes it.
       IF KeepsRecoveryOnFail THEN
         UNCHANGED <<dock, saved, script, restoring>>
       ELSE
         /\ dock' = "K" /\ saved' = Kept(dock) /\ script' = Kept(dock) /\ restoring' = FALSE
  ELSE
    /\ dock' = "K" /\ saved' = Kept(dock) /\ script' = Kept(dock) /\ restoring' = FALSE

\* SystemDock.restore().
DoRestore ==
  IF saved # None THEN
    \/ \* All preferences restored: clear saved state and script.
       /\ dock' = saved /\ saved' = None /\ script' = None /\ restoring' = FALSE
    \/ \* A defaults command failed partway: keep saved state and script until retry succeeds.
       IF KeepsRecoveryOnFail THEN
         /\ dock' = dock /\ saved' = saved /\ script' = script /\ restoring' = TRUE
       ELSE
         /\ dock' = dock /\ saved' = None /\ script' = None /\ restoring' = FALSE
  ELSE IF RecoversFromScript /\ script # None THEN
    \/ \* recover() succeeds.
       /\ dock' = script /\ saved' = None /\ script' = None /\ restoring' = FALSE
    \/ \* recover() fails: keep script on disk so a later launch or watchdog can retry.
       IF KeepsRecoveryOnFail THEN
         /\ dock' = dock /\ saved' = None /\ script' = script /\ restoring' = TRUE
       ELSE
         /\ dock' = dock /\ saved' = None /\ script' = None /\ restoring' = FALSE
  ELSE
    UNCHANGED <<dock, saved, script, restoring>>

\* The two sinks in applicationDidFinishLaunching, with the settings as they are.
Settle(h, r) ==
  /\ IF h THEN DoReplace ELSE DoRestore
  /\ watchdog' = (r \/ (KeepsRecoveryOnFail /\ (saved' # None \/ script' # None)))

Launch ==
  /\ ~running /\ running' = TRUE
  /\ Settle(hides, replaces)
  /\ UNCHANGED <<replaces, visible, armed>>

ToggleReplaces ==
  /\ running /\ replaces' = ~replaces
  /\ Settle(replaces' /\ visible, replaces')
  /\ UNCHANGED <<running, visible, armed>>

ToggleVisible ==
  /\ running /\ visible' = ~visible
  /\ Settle(replaces /\ visible', replaces)
  /\ UNCHANGED <<running, replaces, armed>>

\* Retry a restore that previously failed partway.
RetryRestore ==
  /\ running /\ ~hides /\ restoring
  /\ Settle(FALSE, replaces)
  /\ UNCHANGED <<running, replaces, visible, armed>>

\* Quit, a termination signal, logging out: restore() runs.
Quit ==
  /\ running /\ running' = FALSE
  /\ DoRestore
  /\ UNCHANGED <<replaces, visible, watchdog, armed>>

\* Force quit, crash, power loss: nothing runs.
Crash ==
  /\ running /\ running' = FALSE
  /\ UNCHANGED <<replaces, visible, dock, saved, script, restoring, watchdog, armed>>

\* Kairos's preferences lose the saved values: reset by hand or a cleaner, or a write that
\* never reached the disk before a crash.
PrefsLost ==
  /\ saved # None /\ saved' = None
  /\ UNCHANGED <<running, replaces, visible, dock, script, restoring, watchdog, armed>>

\* The owner changes the Dock setting in System Settings.
OwnerSetsDock ==
  /\ dock' \in {"A", "B"} /\ dock' # dock
  /\ UNCHANGED <<running, replaces, visible, saved, script, restoring, watchdog, armed>>

\* The watchdog's check and its script are two steps: Kairos can start in between.
WatchdogChecks ==
  /\ watchdog /\ ~running /\ script # None /\ ~armed
  /\ armed' = TRUE
  /\ UNCHANGED <<running, replaces, visible, dock, saved, script, restoring, watchdog>>

WatchdogRuns ==
  /\ armed /\ armed' = FALSE
  /\ IF script # None THEN
       \/ /\ dock' = script /\ saved' = None /\ script' = None /\ restoring' = FALSE
       \/ IF KeepsRecoveryOnFail THEN
            UNCHANGED <<dock, saved, script, restoring>>
          ELSE
            /\ dock' = dock /\ saved' = None /\ script' = None /\ restoring' = FALSE
     ELSE
       UNCHANGED <<dock, saved, script, restoring>>
  /\ UNCHANGED <<running, replaces, visible, watchdog>>

Next == Launch \/ ToggleReplaces \/ ToggleVisible \/ RetryRestore \/ Quit \/ Crash \/ PrefsLost
        \/ OwnerSetsDock \/ WatchdogChecks \/ WatchdogRuns

Spec == Init /\ [][Next]_vars

\* Kairos never records its own hidden setting as the owner's: that would make "restore"
\* leave the Mac with no Dock for good.
NeverSavesOwn == saved # "K" /\ script # "K"

\* While the Dock is in Kairos's hidden setting there is something on disk to put it back.
WayBack == dock = "K" => script \in {"A", "B", "D"}

\* A hidden Dock with Kairos stopped always has the watchdog to bring it back.
WatchdogCovers == (dock = "K" /\ ~running) => watchdog

\* With the bar hidden or the switch off while Kairos runs (and not awaiting retry of a failed
\* defaults call), the Dock is back.
BackWhenNotReplacing == (running /\ ~hides /\ ~armed /\ ~restoring) => dock # "K"
=============================================================================
