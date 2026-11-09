---
name: verify-strict-mode
description: "Check a change to Kairos Desktop's mode, Pomodoro timer, 'Allow 5 min' overrides, or quitting and relaunching during Strict against the strict-mode model. Use when editing OverrideLedger (Rules.swift), TempoEngine (Tempo.swift), Mode or FocusState (Models.swift), recoverAfterLaunch (History.swift), setMode / startFocus / skipPhase / stopTimer / apply / syncFocusFromTempo / requestOverride in AppModel.swift, or the Mode picker and Quit handling in KairosApp.swift."
---

# Verify strict mode

Kairos promises that Strict mode cannot be argued with: no "Allow 5 min", and no way out until the timer says so, not even by quitting Kairos. A break in that promise lets the user through the blocker at the moment they asked not to be let through. Unit tests check single calls; the bugs here come from the order of presses (set a mode, start a timer, switch group, skip, quit, relaunch), so a TLA+ model checks every order.

Everything lives in `verification/strict-mode/`:

- `StrictMode.tla` is the model. Each action is one `@MainActor` call in `apps/desktop/Sources/Kairos/AppModel.swift`, which is why each is a single step. Quit, force quit, the saved end passing while Kairos is closed, and relaunch are steps too.
- `anchors.tsv` lists the exact lines of code each part of the model mirrors.
- `expected.tsv` is the result on the code as it stands, including known findings.
- `check.sh` runs it. It needs `java`; it downloads the pinned TLC (TLA+ tools 1.7.4, checksum verified) on first run.

## The properties

| Property | In plain words | Harm if broken |
| --- | --- | --- |
| `StrictNoGrant` | While the mode is Strict there is no live "Allow 5 min" grant. | A blocked site or app stays open in Strict. |
| `StrictNoEscape` | Nothing the user presses takes a strict focus phase out of Strict; only the clock running out does. Quitting, force quitting and relaunching count as presses. | Strict is no stronger than Focus. |
| `QuitRefusedInStrict` | An ordinary Quit (menu bar menu, app menu, Cmd-Q) never goes through during a strict focus phase. | Blocked apps can be reopened until Kairos is launched again. |
| `UsedBounded` | No more than `maxOverridesPerSession` overrides between two ledger resets. | Unlimited overrides. |
| `ModeMatchesTimer` | While a timer runs, the mode sent to the extensions is the mode the timer implies. | Chrome blocks (or fails to block) out of step with the app. |

## Findings so far

None open. Two were found when the model was first run and fixed on 2026-11-09; their guards are now switches in the model, so removing either one fails the check.

- A grant made in Focus used to stay live after switching to Strict. `modeChanged` now resets the ledger on entering Strict (`ResetsOnEnterStrict`).
- Skip and Stop used to end a strict focus phase. `skipPhase` and `stopTimer` now do nothing while `strictLocked` (`LocksStrictControls`), and their buttons are disabled.

Quitting used to end a strict phase: launch always came back off. Since 2026-11-09 the end of a locked strict phase is saved as `FocusState.strictUntil`, launch resumes it (`ResumesStrict`), and Quit is refused while it is locked (`QuitLocked`). Logging out, restarting and shutting down still quit (modelled as `ForceQuit`), and the phase resumes at the next launch. Removing either switch fails the check.

## Run it

```
verification/strict-mode/check.sh            # about 5 seconds
verification/strict-mode/check.sh --mutants  # also proves each property can still fail
```

Exit 0 means the result matches `expected.tsv`. Read the output like this:

- `OUT OF SYNC` (exit 2): a line the model mirrors is gone or duplicated. The model no longer describes the code. Go to "When the code changes".
- `NOTE covered files changed`: the anchors still match but a covered file was edited. Read the diff for a new way to change `data.focus.mode`, the `TempoEngine` or the `OverrideLedger` (a new button, a new bridge message, a new call to `ledger.reset()`). If there is one, add it to the model as an action. Then run `check.sh --confirm`.
- `CHANGED` (exit 1): a property's result differs from `expected.tsv`. The steps printed under it are the shortest sequence of presses that breaks it, with the mode, timer phase and grant after each.
- `KNOWN FINDING`: a property recorded as `FAIL` in `expected.tsv`, unchanged. There are none today.

Read every result with the `/statechart` skill. It tells each violation as its shortest path in plain words and shows which violations a change fixes; when everything passes it draws how the app moves between states.

## Real bug or modeling error?

Take the printed steps and walk them through the code, one line per step, using `anchors.tsv` to find each line. If every step is something the code really does, it is a real bug: report it with the steps in the code's words, and do not change product code unless asked. If a step cannot happen (a button is disabled, a guard returns early), the model is missing that guard: add it to the action, add its line to `anchors.tsv`, and rerun.

## When the code changes

1. Make the model say what the code now says. A removed guard means the matching switch is false for the current code (for example `LedgerRefusesStrict`); set it in the `run` defaults in `check.sh`. A new way to change mode, timer or overrides is a new action in `Next`.
2. Update `anchors.tsv` so each mirrored line is listed with its real count.
3. Run `check.sh --mutants`. If a property now fails that used to pass, that is the result of the change: report it, or fix the code if that was the task.
4. If the check finds a new break you are not fixing now, record it: set that row in `expected.tsv` to `FAIL` with a one-line note, and describe it under Findings with its steps.
5. Only after you have read the covered files and the model side by side, run `check.sh --confirm`.

## Coverage and sync

Covered: `OverrideLedger` in `apps/desktop/Sources/KairosCore/Rules.swift`; `TempoEngine.mode`, `use`, `start`, `skip`, `stop`, `tick`, `advance`, `reset`, `resumeStrict` in `Tempo.swift`; `Mode.blocks` and `FocusState.strictUntil` in `Models.swift`; `AppData.recoverAfterLaunch` in `History.swift`; `setMode`, `modeChanged`, `strictLocked`, `startFocus`, `requestStartFocus`, `requestSkip`, `toggleTimer`, `skipPhase`, `stopTimer`, `apply`, `syncFocusFromTempo`, `switchGroup`, `startTask`, `requestOverride` and the launch in `init` in `apps/desktop/Sources/Kairos/AppModel.swift`; `ModePicker` (the one Mode control, used in the menu bar menu and on Today), the Quit buttons and `applicationShouldTerminate` in `KairosApp.swift`.

`requestStartFocus` and `requestSkip` ask before a strict focus phase starts. Agreeing is the model's `StartFocus` or `Skip`; cancelling changes nothing, so it is no step.

Last confirmed in sync: 2026-11-09, recorded as file checksums in `verification/strict-mode/synced.sha256`, in the commit "Start the timer from a task and change the mode from Today" (its parent is fedf001). Write the commit hash here each time you run `--confirm`.

## What this does not cover

- It checks the model, not the Swift. `anchors.tsv` catches a mirrored line changing; it cannot catch new code that changes the mode some other way. That is what the `NOTE` is for.
- Bounds: `Max = 3` overrides. Short and long breaks are one phase. Grant expiry is "at some point", not five minutes.
- Quit and relaunch: a paused strict phase resumes running at launch, with its saved end (the time it would have ended if resumed at the last save); the model treats paused and running alike there.
- Left out: the Chrome extensions' own handling of the pushed state (they keep enforcing until `strictUntil` while Kairos is closed), app blocking and Jev, and the reason text on an override.
