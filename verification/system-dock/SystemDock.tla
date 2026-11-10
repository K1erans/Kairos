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
  RefusesOwnValues     \* replace() never records Kairos's own hidden setting as the owner's

VARIABLES running, replaces, visible, dock, saved, script, watchdog, armed

vars == <<running, replaces, visible, dock, saved, script, watchdog, armed>>
Values == {"A", "B", "K", "D"}
None == "none"

Init ==
  /\ running = FALSE /\ replaces = FALSE /\ visible = TRUE
  /\ dock = "A" /\ saved = None /\ script = None
  /\ watchdog = FALSE /\ armed = FALSE

hides == replaces /\ visible

\* What the leftover script does: put its value back and remove itself.
\* (state after running it, as dock/script; saved is cleared by the script too)
ScriptDock == IF script # None THEN script ELSE dock

\* SystemDock.replace(), as the next values of <<dock, saved, script>>.
Replaced ==
  IF saved # None THEN <<dock, saved, saved>>               \* already hidden: rewrite the script
  ELSE LET base == IF RecoversFromScript THEN ScriptDock ELSE dock
           keep == IF RefusesOwnValues /\ base = "K" THEN "D" ELSE base
       IN <<"K", keep, keep>>

\* SystemDock.restore().
Restored ==
  IF saved # None THEN <<saved, None, None>>
  ELSE IF RecoversFromScript /\ script # None THEN <<script, None, None>>
  ELSE <<dock, saved, script>>

Apply(next) == /\ dock' = next[1] /\ saved' = next[2] /\ script' = next[3]

\* The two sinks in applicationDidFinishLaunching, with the settings as they are.
Settle(h, r) ==
  /\ Apply(IF h THEN Replaced ELSE Restored)
  /\ watchdog' = r

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

\* Quit, a termination signal, logging out: restore() runs.
Quit ==
  /\ running /\ running' = FALSE
  /\ Apply(Restored)
  /\ UNCHANGED <<replaces, visible, watchdog, armed>>

\* Force quit, crash, power loss: nothing runs.
Crash ==
  /\ running /\ running' = FALSE
  /\ UNCHANGED <<replaces, visible, dock, saved, script, watchdog, armed>>

\* Kairos's preferences lose the saved values: reset by hand or a cleaner, or a write that
\* never reached the disk before a crash.
PrefsLost ==
  /\ saved # None /\ saved' = None
  /\ UNCHANGED <<running, replaces, visible, dock, script, watchdog, armed>>

\* The owner changes the Dock setting in System Settings.
OwnerSetsDock ==
  /\ dock' \in {"A", "B"} /\ dock' # dock
  /\ UNCHANGED <<running, replaces, visible, saved, script, watchdog, armed>>

\* The watchdog's check and its script are two steps: Kairos can start in between.
WatchdogChecks ==
  /\ watchdog /\ ~running /\ script # None /\ ~armed
  /\ armed' = TRUE
  /\ UNCHANGED <<running, replaces, visible, dock, saved, script, watchdog>>

WatchdogRuns ==
  /\ armed /\ armed' = FALSE
  /\ IF script # None THEN Apply(<<script, None, None>>) ELSE UNCHANGED <<dock, saved, script>>
  /\ UNCHANGED <<running, replaces, visible, watchdog>>

Next == Launch \/ ToggleReplaces \/ ToggleVisible \/ Quit \/ Crash \/ PrefsLost
        \/ OwnerSetsDock \/ WatchdogChecks \/ WatchdogRuns

Spec == Init /\ [][Next]_vars

\* Kairos never records its own hidden setting as the owner's: that would make "restore"
\* leave the Mac with no Dock for good.
NeverSavesOwn == saved # "K" /\ script # "K"

\* While the Dock is in Kairos's hidden setting there is something on disk to put it back.
WayBack == dock = "K" => script \in {"A", "B", "D"}

\* A hidden Dock with Kairos stopped always has the watchdog to bring it back.
WatchdogCovers == (dock = "K" /\ ~running) => watchdog

\* With the bar hidden or the switch off while Kairos runs, the Dock is back.
BackWhenNotReplacing == (running /\ ~hides /\ ~armed) => dock # "K"
=============================================================================
