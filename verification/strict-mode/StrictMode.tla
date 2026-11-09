------------------------------ MODULE StrictMode ------------------------------
(* How Kairos Desktop's mode, Pomodoro timer and "Allow 5 min" overrides move
   together. Every action is one @MainActor call in
   apps/desktop/Sources/Kairos/AppModel.swift, so each is one indivisible step.
   Quitting and relaunching Kairos are steps too: the app saves the end of a
   locked strict phase (FocusState.strictUntil) and launch picks it up again.
   Each switch below mirrors one line of code listed in anchors.tsv. *)
EXTENDS Naturals

CONSTANTS
    Max,                  \* Settings.maxOverridesPerSession
    LedgerRefusesStrict,  \* OverrideLedger.request: guard mode != .strict
    LedgerChecksMax,      \* OverrideLedger.request: guard used < max
    SyncsModeFromTimer,   \* AppModel.syncFocusFromTempo copies tempo.mode
    PickerLockedByTimer,  \* KairosApp: Mode picker .disabled(tempo.phase != .idle)
    ResetsOnEnterStrict,  \* AppModel.modeChanged: reset the ledger on entering strict
    LocksStrictControls,  \* AppModel.strictLocked: Skip and Stop do nothing in a strict focus phase
    QuitLocked,           \* AppDelegate.applicationShouldTerminate and the Quit buttons refuse in a strict focus phase
    ResumesStrict         \* AppData.recoverAfterLaunch + TempoEngine.resumeStrict: launch picks up a saved strict phase

VARIABLES
    phase,       \* TempoEngine.phase: "idle", "focus", "rest" (short and long break alike)
    paused,      \* TempoEngine.isPaused
    holds,       \* TempoEngine.holdsStrict
    preset,      \* TempoEngine.preset.strict
    pending,     \* TempoEngine.pendingPreset: "none", "strict", "plain"
    groupStrict, \* the active group's preset is strict
    mode,        \* data.focus.mode: "off", "focus", "break", "strict"
    used,        \* OverrideLedger.used
    grant,       \* OverrideLedger holds an unexpired allow
    escaped,     \* history: a user action took a strict focus phase out of strict
    up,          \* Kairos is running
    downStrict,  \* Kairos is not running and the saved strictUntil has not passed
    quitInStrict,\* history: an ordinary Quit went through during a strict focus phase
    act          \* history: name of the last step, for reading traces

vars == <<phase, paused, holds, preset, pending, groupStrict, mode, used, grant, escaped, up, downStrict, quitInStrict, act>>

Blocks(m) == m \in {"focus", "strict"}                     \* Mode.blocks

TempoMode(ph, h, p) ==                                     \* TempoEngine.mode
    IF ph = "idle" THEN "off"
    ELSE IF ph = "focus" THEN (IF h \/ p THEN "strict" ELSE "focus")
    ELSE "break"

PresetAfterPending == IF pending = "none" THEN preset ELSE pending = "strict"   \* takePendingPreset

InStrictPhase == phase = "focus" /\ mode = "strict"

\* AppModel.syncFocusFromTempo
Synced(ph, h, p, old) == IF SyncsModeFromTimer /\ ph # "idle" THEN TempoMode(ph, h, p) ELSE old

\* Ledger after a step: apply() resets on .started(focus); modeChanged resets when blocking turns on.
Ledger(startedFocus, old, new) ==
    IF startedFocus \/ (Blocks(new) /\ ~Blocks(old)) \/ (ResetsOnEnterStrict /\ new = "strict" /\ old # "strict")
    THEN /\ used' = 0 /\ grant' = FALSE
    ELSE UNCHANGED <<used, grant>>

Init ==
    /\ phase = "idle" /\ paused = FALSE /\ holds = FALSE /\ preset = FALSE /\ pending = "none"
    /\ groupStrict = FALSE /\ mode = "off" /\ used = 0 /\ grant = FALSE /\ escaped = FALSE
    /\ up = TRUE /\ downStrict = FALSE /\ quitInStrict = FALSE
    /\ act = "Launch"                                      \* AppData.recoverAfterLaunch: nothing saved, mode off

\* The Mode picker (one ModePicker view, in the menu bar menu and on Today) -> AppModel.setMode
SetMode(m) ==
    /\ m # mode
    /\ PickerLockedByTimer => phase = "idle"
    /\ mode' = m
    /\ Ledger(FALSE, mode, m)
    /\ escaped' = (escaped \/ (InStrictPhase /\ m # "strict"))
    /\ act' = "SetMode"
    /\ UNCHANGED <<phase, paused, holds, preset, pending, groupStrict>>

\* AppModel.startFocus: tempo.use(group preset), tempo.start(.focus, strict: mode == .strict).
\* "Start" on a task (AppModel.startTask) is SwitchGroup to the task's group, then this.
StartFocus ==
    /\ phase = "idle"
    /\ preset' = groupStrict /\ pending' = "none"
    /\ holds' = (mode = "strict")
    /\ phase' = "focus" /\ paused' = FALSE
    /\ mode' = Synced("focus", holds', preset', mode)
    /\ Ledger(TRUE, mode, mode')
    /\ act' = "StartFocus"
    /\ UNCHANGED <<groupStrict, escaped>>

\* AppModel.toggleTimer while running or paused
Pause ==
    /\ phase # "idle" /\ ~paused
    /\ paused' = TRUE /\ act' = "Pause"
    /\ UNCHANGED <<phase, holds, preset, pending, groupStrict, mode, used, grant, escaped>>

Resume ==
    /\ paused
    /\ paused' = FALSE /\ act' = "Resume"
    /\ UNCHANGED <<phase, holds, preset, pending, groupStrict, mode, used, grant, escaped>>

\* TempoEngine.advance: the next phase, taking any pending preset
Advance(name, byUser) ==
    /\ preset' = PresetAfterPending /\ pending' = "none"
    /\ phase' = (IF phase = "focus" THEN "rest" ELSE "focus")
    /\ paused' = FALSE
    /\ mode' = Synced(phase', holds, preset', mode)
    /\ Ledger(phase' = "focus", mode, mode')
    /\ escaped' = (escaped \/ (byUser /\ InStrictPhase /\ mode' # "strict"))
    /\ act' = name
    /\ UNCHANGED <<holds, groupStrict>>

\* TempoEngine.stop / reset, then apply(.stopped or .awaitingFocus): data.focus.mode = .off
Halt(name, byUser) ==
    /\ preset' = PresetAfterPending /\ pending' = "none"
    /\ phase' = "idle" /\ paused' = FALSE /\ holds' = FALSE
    /\ mode' = "off"
    /\ Ledger(FALSE, mode, "off")
    /\ escaped' = (escaped \/ (byUser /\ InStrictPhase))
    /\ act' = name
    /\ UNCHANGED groupStrict

\* AppModel.skipPhase and AppModel.stopTimer: the Skip and Stop buttons
Skip == phase # "idle" /\ (LocksStrictControls => ~InStrictPhase) /\ Advance("Skip", TRUE)
Stop == phase # "idle" /\ (LocksStrictControls => ~InStrictPhase) /\ Halt("Stop", TRUE)

\* AppModel.tick: the phase ran out; after a long sleep the timer stops instead. With "Start the
\* next focus session automatically" off, a break that runs out stops the timer to wait
\* (TempoEngine.advance, waitsAfterBreak; apply(.awaitingFocus) turns the mode off as .stopped does).
TimeUp     == phase # "idle" /\ ~paused /\ (Advance("TimeUp", FALSE) \/ (phase = "rest" /\ Halt("BreakOver", FALSE)))
LongSleep  == phase # "idle" /\ ~paused /\ Halt("LongSleep", FALSE)

\* AppModel.switchGroup -> TempoEngine.use
SwitchGroup(s) ==
    /\ groupStrict' = s
    /\ IF phase = "idle" THEN preset' = s /\ pending' = "none"
       ELSE pending' = (IF s THEN "strict" ELSE "plain") /\ UNCHANGED preset
    /\ act' = "SwitchGroup"
    /\ UNCHANGED <<phase, paused, holds, mode, used, grant, escaped>>

\* AppModel.requestOverride -> OverrideLedger.request (the reason is typed, so always present)
Override ==
    /\ IF LedgerRefusesStrict THEN mode = "focus" ELSE Blocks(mode)
    /\ LedgerChecksMax => used < Max
    /\ used' = used + 1 /\ grant' = TRUE /\ act' = "Override"
    /\ UNCHANGED <<phase, paused, holds, preset, pending, groupStrict, mode, escaped>>

\* Five minutes pass: OverrideLedger.isAllowed compares the expiry with now
GrantExpires ==
    /\ grant /\ grant' = FALSE /\ act' = "GrantExpires"
    /\ UNCHANGED <<phase, paused, holds, preset, pending, groupStrict, mode, used, escaped>>

\* Quit Kairos from the menu bar menu, the app menu or Cmd-Q: all reach applicationShouldTerminate.
Quit ==
    /\ up
    /\ QuitLocked => ~InStrictPhase
    /\ up' = FALSE
    /\ downStrict' = InStrictPhase                         \* syncFocusFromTempo saved strictUntil
    /\ quitInStrict' = (quitInStrict \/ InStrictPhase)
    /\ act' = "Quit"
    /\ UNCHANGED <<phase, paused, holds, preset, pending, groupStrict, mode, used, grant, escaped>>

\* Force quit, a crash, a termination signal (kill, Ctrl-C: AppDelegate.endOnSignal saves and exits),
\* or logging out and shutting down (which applicationShouldTerminate lets through): nothing in
\* Kairos can refuse these.
ForceQuit ==
    /\ up
    /\ up' = FALSE
    /\ downStrict' = InStrictPhase
    /\ act' = "ForceQuit"
    /\ UNCHANGED <<phase, paused, holds, preset, pending, groupStrict, mode, used, grant, escaped, quitInStrict>>

\* The saved strictUntil passes while Kairos is not running.
TimeUpWhileQuit ==
    /\ ~up /\ downStrict
    /\ downStrict' = FALSE
    /\ act' = "TimeUpWhileQuit"
    /\ UNCHANGED <<phase, paused, holds, preset, pending, groupStrict, mode, used, grant, escaped, up, quitInStrict>>

\* Launch: a fresh TempoEngine with the group's preset and a fresh ledger. A strict phase whose
\* saved end is still ahead resumes, holding strict; anything else comes back off.
Relaunch ==
    /\ ~up
    /\ up' = TRUE
    /\ preset' = groupStrict /\ pending' = "none" /\ paused' = FALSE
    /\ IF ResumesStrict /\ downStrict
       THEN phase' = "focus" /\ holds' = TRUE /\ mode' = "strict"
       ELSE phase' = "idle" /\ holds' = FALSE /\ mode' = "off"
    /\ used' = 0 /\ grant' = FALSE
    /\ escaped' = (escaped \/ (downStrict /\ mode' # "strict"))
    /\ downStrict' = FALSE
    /\ act' = "Relaunch"
    /\ UNCHANGED <<groupStrict, quitInStrict>>

\* Everything the running app does.
AppStep ==
    \/ \E m \in {"off", "focus", "break", "strict"} : SetMode(m)
    \/ StartFocus \/ Pause \/ Resume \/ Skip \/ Stop \/ TimeUp \/ LongSleep
    \/ \E s \in BOOLEAN : SwitchGroup(s)
    \/ Override \/ GrantExpires

Next ==
    \/ up /\ AppStep /\ UNCHANGED <<up, downStrict, quitInStrict>>
    \/ Quit \/ ForceQuit \/ TimeUpWhileQuit \/ Relaunch

Spec == Init /\ [][Next]_vars

\* The explorer runs a mutant with LedgerChecksMax off; this keeps its state space finite.
Bound == used <= Max + 1

-------------------------------------------------------------------------------
\* Strict mode never has a live "Allow 5 min" grant.
StrictNoGrant == mode = "strict" => ~grant

\* Nothing the user presses takes a strict focus phase out of strict; only the clock does.
\* Quitting, force quitting and relaunching count as presses.
StrictNoEscape == ~escaped

\* An ordinary Quit never goes through during a strict focus phase.
QuitRefusedInStrict == ~quitInStrict

\* No more than Max overrides between two ledger resets.
UsedBounded == used <= Max

\* While a timer runs, the mode the extensions see is the mode the timer implies.
ModeMatchesTimer == phase # "idle" => mode = TempoMode(phase, holds, preset)
===============================================================================
