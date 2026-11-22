--------------------------- MODULE CinnamonPanel ---------------------------
(* The Linux counterpart of SystemDock.tla: Kairos hides Cinnamon's panels and promises to put
   back exactly what was there. Mirrors replace(), restore() and the watchdog in
   apps/linux/crates/kairos-core/src/panel.rs, and sync_panel(), finish(), the signal handlers
   and `--restore-panel` in apps/linux/crates/kairos/src/main.rs.

   Same names as SystemDock.tla where the meaning carries over:
     dock      the org.cinnamon panels-autohide / panels-show-delay pair. "A" is what the owner
               had, "B" something else the owner picked later, "K" Kairos's own setting (every
               panel auto-hides, show delay 2000), "D" the schema default (gsettings reset).
     saved     panel-saved.json in the data directory.
     script    restore-panel.sh: what it would put back.
     watchdog  the kairos-panel-watchdog systemd user timer, enabled.
     armed     the script has passed its `pgrep -x kairos` check and not yet run gsettings.
   Linux only:
     removing  the script has run gsettings and not yet run `rm panel-saved.json`.
     hidden    App::panel_hidden, in memory: false at every launch.
     owned     ghost: Kairos has put "K" on the panel and not yet put anything back.
     undone    ghost: Kairos has written over the panel at a time it was not holding it hidden.

   Where Linux differs from the Mac model:
   - sync_panel() runs at launch and on every DATA change, and does nothing when the wanted
     state equals both panel_hidden and is_replaced() (panel-saved.json exists).
   - The timer is installed by replace() and removed by restore(), not by the switch.
   - finish() calls restore() only if panel_hidden is set.
   - restore() always removes the script and the timer, even with nothing saved.
   - replace() with values saved hides the panel again (the Mac one only rewrites the script).
   - The script does not remove itself: it removes panel-saved.json and stays, and the timer
     runs it again every 30 seconds while no `kairos` process runs.
   - The script's gsettings calls and its `rm` are separate steps (removing), since its pgrep
     check is only at the start and a Kairos starting in between does not stop it. *)
EXTENDS Naturals

CONSTANTS
  RecoversFromScript,   \* replace()/restore() with nothing saved run a leftover script first (with --force)
  RefusesOwnValues,     \* replace() never records Kairos's own setting as the owner's
  ScriptRemovesItself,  \* restore-panel.sh deletes itself after putting the values back
  ScriptLocked          \* the script and replace()/restore() share a lock (flock), so a Kairos
                        \* that starts while the script runs waits for it to finish

VARIABLES running, replaces, visible, dock, saved, script, watchdog, armed, removing, hidden, owned, undone

vars == <<running, replaces, visible, dock, saved, script, watchdog, armed, removing, hidden, owned, undone>>
None == "none"

Init ==
  /\ running = FALSE /\ replaces = FALSE /\ visible = TRUE
  /\ dock = "A" /\ saved = None /\ script = None
  /\ watchdog = FALSE /\ armed = FALSE /\ removing = FALSE /\ hidden = FALSE
  /\ owned = FALSE /\ undone = FALSE

\* Settings::hides_system_dock() (with an X11 display, which sync_panel also needs).
hides == replaces /\ visible

\* The restore script is between its pgrep check and its last step.
scriptBusy == armed \/ removing

\* replace()/restore() can run now: always, unless they wait on the script's lock.
MayTouchPanel == ~ScriptLocked \/ ~scriptBusy

\* Kairos writes `v` to the panel. It overrides the owner unless Kairos was holding it hidden.
PutBack(v) ==
  /\ undone' = (undone \/ (~owned /\ v # dock))
  /\ owned' = FALSE

\* panel::replace(): the next <<dock, saved, script>>; the timer is enabled.
Replace ==
  LET recover == saved = None /\ RecoversFromScript /\ script # None
      base == IF recover THEN script ELSE dock
      keep == IF saved # None THEN saved
              ELSE IF RefusesOwnValues /\ base = "K" THEN "D" ELSE base
  IN /\ dock' = "K" /\ saved' = keep /\ script' = keep /\ watchdog' = TRUE
     /\ undone' = (undone \/ (recover /\ ~owned /\ script # dock))
     /\ owned' = TRUE

\* panel::restore(): puts back what is saved, then remove_watchdog() deletes the timer and the
\* script whether or not anything was put back.
Restore ==
  /\ saved' = None /\ script' = None /\ watchdog' = FALSE
  /\ IF saved # None THEN dock' = saved /\ PutBack(saved)
     ELSE IF RecoversFromScript /\ script # None THEN dock' = script /\ PutBack(script)
     ELSE UNCHANGED <<dock, owned, undone>>

\* App::sync_panel() with the wanted state `h`.
Sync(h) ==
  IF h = hidden /\ h = (saved # None)
  THEN UNCHANGED <<dock, saved, script, watchdog, hidden, owned, undone>>
  ELSE IF h THEN Replace /\ hidden' = TRUE
  ELSE Restore /\ hidden' = FALSE

Launch ==
  /\ ~running /\ running' = TRUE /\ MayTouchPanel
  /\ Sync(hides)
  /\ UNCHANGED <<replaces, visible, armed, removing>>

\* The settings switch (pages.rs), through model.edit and a DATA change.
ToggleReplaces ==
  /\ running /\ replaces' = ~replaces
  /\ Sync(replaces' /\ visible)
  /\ UNCHANGED <<running, visible, armed, removing>>

\* "Show Kairos Bar" in Settings or the tray.
ToggleVisible ==
  /\ running /\ visible' = ~visible
  /\ Sync(replaces /\ visible')
  /\ UNCHANGED <<running, replaces, armed, removing>>

\* Any other DATA change runs sync_panel() again with the same settings.
DataChanged ==
  /\ running
  /\ Sync(hides)
  /\ UNCHANGED <<running, replaces, visible, armed, removing>>

\* Quit from the tray, SIGINT/SIGTERM/SIGHUP, logout: finish() restores only if panel_hidden.
Quit ==
  /\ running /\ running' = FALSE /\ hidden' = FALSE
  /\ IF hidden THEN Restore ELSE UNCHANGED <<dock, saved, script, watchdog, owned, undone>>
  /\ UNCHANGED <<replaces, visible, armed, removing>>

\* SIGKILL, a crash, power loss: nothing runs, and panel_hidden is gone with the process.
Crash ==
  /\ running /\ running' = FALSE /\ hidden' = FALSE
  /\ UNCHANGED <<replaces, visible, dock, saved, script, watchdog, armed, removing, owned, undone>>

\* `kairos --restore-panel`, by hand or from install.sh --uninstall: panel::restore() in a
\* second process. The running Kairos, if any, keeps its panel_hidden.
RestoreCommand ==
  /\ MayTouchPanel /\ Restore
  /\ UNCHANGED <<running, replaces, visible, armed, removing, hidden>>

\* panel-saved.json is gone or unreadable: deleted by hand, or a failed parse in load_saved().
\* (The script's own `rm` after a Kairos that started mid-script is modelled as itself.)
PrefsLost ==
  /\ saved # None /\ saved' = None
  /\ UNCHANGED <<running, replaces, visible, dock, script, watchdog, armed, removing, hidden, owned, undone>>

\* The owner changes the panel's auto-hide in Cinnamon's panel settings.
OwnerSetsDock ==
  /\ dock' \in {"A", "B"} /\ dock' # dock
  /\ UNCHANGED <<running, replaces, visible, saved, script, watchdog, armed, removing, hidden, owned, undone>>

\* The timer fires and the script's pgrep finds no kairos: Kairos can start before it goes on.
WatchdogChecks ==
  /\ watchdog /\ ~running /\ script # None /\ ~scriptBusy
  /\ armed' = TRUE
  /\ UNCHANGED <<running, replaces, visible, dock, saved, script, watchdog, removing, hidden, owned, undone>>

\* The script's two gsettings calls.
WatchdogRuns ==
  /\ armed /\ armed' = FALSE
  /\ IF script # None
     THEN /\ dock' = script /\ removing' = TRUE
          /\ undone' = (undone \/ (~owned /\ script # dock))
     ELSE UNCHANGED <<dock, removing, undone>>
  /\ UNCHANGED <<running, replaces, visible, saved, script, watchdog, hidden, owned>>

\* The script's `rm panel-saved.json` (and, with the fix, `rm "$0"`). Kairos stops holding the
\* panel here, unless a Kairos that started mid-script has hidden it again.
WatchdogRemoves ==
  /\ removing /\ removing' = FALSE /\ saved' = None
  /\ script' = IF ScriptRemovesItself THEN None ELSE script
  /\ owned' = (owned /\ dock = "K")
  /\ UNCHANGED <<running, replaces, visible, dock, watchdog, armed, hidden, undone>>

Next == Launch \/ ToggleReplaces \/ ToggleVisible \/ DataChanged \/ Quit \/ Crash
        \/ RestoreCommand \/ PrefsLost \/ OwnerSetsDock \/ WatchdogChecks \/ WatchdogRuns
        \/ WatchdogRemoves

Spec == Init /\ [][Next]_vars

\* Kairos never records its own setting as the owner's: "restore" would then leave every panel
\* hidden for good.
NeverSavesOwn == saved # "K" /\ script # "K"

\* While the panel is in Kairos's setting, restore-panel.sh holds a real setting to put back.
WayBack == dock = "K" => script \in {"A", "B", "D"}

\* A hidden panel with Kairos stopped always has the timer to bring it back.
WatchdogCovers == (dock = "K" /\ ~running) => watchdog

\* With Kairos Bar hidden or the switch off while Kairos runs, the panel is back.
BackWhenNotReplacing == (running /\ ~hides /\ ~scriptBusy) => dock # "K"

\* Linux only: once the panel is put back, Kairos leaves the owner's later changes alone.
LeavesOwnersPanel == ~undone
=============================================================================
