---
name: verify-strict-mode
description: "Check a change to Kairos Desktop's mode, Pomodoro timer, 'Allow 5 min' overrides, blocking-rule edits, or quitting and relaunching during Strict against the strict-mode model. Use when editing OverrideLedger, RuleLock or AppData.updateGroup (Rules.swift), TempoEngine (Tempo.swift), Mode or FocusState (Models.swift), recoverAfterLaunch (History.swift), setMode / startFocus / skipPhase / stopTimer / apply / syncFocusFromTempo / requestOverride in AppModel.swift, or the Mode picker and Quit handling in KairosApp.swift. Also for the same code in Kairos for Linux (apps/linux, Rust): OverrideLedger, rule_lock and AppData::update_group (rules.rs), TempoEngine (tempo.rs), Mode and FocusState (models.rs), recover_after_launch (history.rs), set_mode / start_focus / skip_phase / stop_timer / apply / sync_focus_from_tempo / switch_group / request_override in kairos/src/model.rs, the tray menu and App::quit (main.rs, platform/tray.rs), and the Mode picker and Skip/Stop on Today (ui/pages.rs)."
---

# Verify strict mode

Kairos promises that Strict mode cannot be argued with: no "Allow 5 min", no loosening the rules, and no way out until the timer says so, not even by quitting Kairos. A break in that promise lets the user through the blocker at the moment they asked not to be let through. Unit tests check single calls; the bugs here come from the order of presses (set a mode, start a timer, switch group, skip, quit, relaunch), so a TLA+ model checks every order.

Everything lives in `verification/strict-mode/`:

- `StrictMode.tla` is the model. Each action is one `@MainActor` call in `apps/desktop/Sources/Kairos/AppModel.swift`, which is why each is a single step. Quit, force quit, the saved end passing while Kairos is closed, and relaunch are steps too.
- `anchors.tsv` lists the exact lines of code each part of the model mirrors.
- `expected.tsv` is the result on the code as it stands, including known findings.
- `check.sh` runs it. It needs `java`; it downloads the pinned TLC (TLA+ tools 1.7.4, checksum verified) on first run.
- `anchors-linux.tsv`, `expected-linux.tsv` and `synced-linux.sha256` are the same three for Kairos for Linux (`apps/linux`, Rust). The Rust port does what the Swift does in every part the model describes, so the same `StrictMode.tla`, with the same switch values, checks both. See "Kairos for Linux" below.

## The properties

| Property | In plain words | Harm if broken |
| --- | --- | --- |
| `StrictNoGrant` | While the mode is Strict there is no live "Allow 5 min" grant. | A blocked site or app stays open in Strict. |
| `StrictNoEscape` | Nothing the user presses takes a strict focus phase out of Strict; only the clock running out does. Quitting, force quitting and relaunching count as presses. | Strict is no stronger than Focus. |
| `QuitRefusedInStrict` | An ordinary Quit (menu bar menu, app menu, Cmd-Q) never goes through during a strict focus phase. | Blocked apps can be reopened until Kairos is launched again. |
| `UsedBounded` | No more than `maxOverridesPerSession` overrides between two ledger resets. | Unlimited overrides. |
| `ModeMatchesTimer` | While a timer runs, the mode sent to the extensions is the mode the timer implies. | Chrome blocks (or fails to block) out of step with the app. |
| `StrictRulesHold` | During a strict focus phase no edit loosens the active group's rules (unblock a site or app, allow a site, show a hidden element, turn Jev off or raise its threshold). Adding blocks is fine. | A blocked site is unblocked on the Rules page or from Kairos Shield, and Strict is no stronger than Focus. |
| `StrictKeepsGroup` | During a strict focus phase no group switch puts looser rules in force. | Pressing another group's shortcut lifts the blocking. |

## Findings so far

One open, recorded as `FAIL` in `expected.tsv` and in `expected-linux.tsv` (the Rust port has it too, see "Kairos for Linux"):

- `StrictKeepsGroup` (found 2026-11-10, when rule edits were added to the model). `switchGroup` has no strict guard, so during a strict focus phase a group switch (Kairos Bar, ⌃1 to ⌃9, Start on a task in another group, or deleting the active group) puts that group's rules in force even when it blocks less. Steps: SetMode(strict), StartFocus, SwitchGroup to a looser group. Whether to refuse it is the owner's call: `RuleLock.loosens(active, to: next)` could refuse only switches to a looser group, but most pairs of groups are not subsets of each other, so in practice that refuses most switches.

Two were found when the model was first run and fixed on 2026-11-09; their guards are now switches in the model, so removing either one fails the check.

- A grant made in Focus used to stay live after switching to Strict. `modeChanged` now resets the ledger on entering Strict (`ResetsOnEnterStrict`).
- Skip and Stop used to end a strict focus phase. `skipPhase` and `stopTimer` now do nothing while `strictLocked` (`LocksStrictControls`), and their buttons are disabled.

Since 2026-11-10 the Rules page, Kairos Shield's `rule.addSite` and `rule.removeElement`, and Kairos Bar's Unblock cannot loosen the active group's rules during a strict focus phase: every rule change goes through `AppData.updateGroup`, which refuses a change `RuleLock.loosens` flags (`LocksRules`). The Rules page disables those controls and says when they unlock.

Quitting used to end a strict phase: launch always came back off. Since 2026-11-09 the end of a locked strict phase is saved as `FocusState.strictUntil`, launch resumes it (`ResumesStrict`), and Quit is refused while it is locked (`QuitLocked`). Logging out, restarting and shutting down still quit (modelled as `ForceQuit`), and the phase resumes at the next launch. Removing either switch fails the check.

## Run it

```
verification/strict-mode/check.sh                    # Mac (Swift), about 5 seconds
verification/strict-mode/check.sh --mutants          # also proves each property can still fail
verification/strict-mode/check.sh --linux --mutants  # Linux (Rust), against anchors-linux.tsv and expected-linux.tsv
verification/strict-mode/check.sh --all --mutants    # both, Mac first, each under its own heading
```

With no target flag it checks the Mac app only, and the output is as it always was. Exit 0 means the result matches `expected.tsv` (or `expected-linux.tsv`); with `--all` the exit code is the worse of the two. Read the output like this:

- `OUT OF SYNC` (exit 2): a line the model mirrors is gone or duplicated. The model no longer describes the code. Go to "When the code changes".
- `NOTE covered files changed`: the anchors still match but a covered file was edited. Read the diff for a new way to change `data.focus.mode`, the `TempoEngine`, the `OverrideLedger` or a group's rules (a new button, a new bridge message, a new call to `ledger.reset()`, a write to `data.groups` that skips `updateGroup`). If there is one, add it to the model as an action. Then run `check.sh --confirm`.
- `CHANGED` (exit 1): a property's result differs from `expected.tsv`. The steps printed under it are the shortest sequence of presses that breaks it, with the mode, timer phase and grant after each.
- `KNOWN FINDING`: a property recorded as `FAIL` in `expected.tsv`, unchanged. Today that is `StrictKeepsGroup`, on both targets.

Read every result with the `/statechart` skill. It tells each violation as its shortest path in plain words and shows which violations a change fixes; when everything passes it draws how the app moves between states.

## Real bug or modeling error?

Take the printed steps and walk them through the code, one line per step, using `anchors.tsv` to find each line. If every step is something the code really does, it is a real bug: report it with the steps in the code's words, and do not change product code unless asked. If a step cannot happen (a button is disabled, a guard returns early), the model is missing that guard: add it to the action, add its line to `anchors.tsv`, and rerun.

## When the code changes

1. Make the model say what the code now says. A removed guard means the matching switch is false for the current code (for example `LedgerRefusesStrict`); set it in that target's `SWITCHES` in `use_target` in `check.sh` (Mac and Linux are set apart, so one app losing a guard does not change the other's run). A new way to change mode, timer or overrides is a new action in `Next`.
2. Update `anchors.tsv` so each mirrored line is listed with its real count.
3. Run `check.sh --mutants`. If a property now fails that used to pass, that is the result of the change: report it, or fix the code if that was the task.
4. If the check finds a new break you are not fixing now, record it: set that row in `expected.tsv` to `FAIL` with a one-line note, and describe it under Findings with its steps.
5. Only after you have read the covered files and the model side by side, run `check.sh --confirm` (Mac) or `check.sh --confirm --linux`.

## Coverage and sync

Covered: `OverrideLedger`, `RuleLock`, `AppData.updateGroup`, `addSite`, `setAppBlocked` and `setElementHidden` in `apps/desktop/Sources/KairosCore/Rules.swift`; `TempoEngine.mode`, `use`, `start`, `skip`, `stop`, `tick`, `advance` (including the wait after a break), `endWait`, `reset`, `resumeStrict` in `Tempo.swift`; `Mode.blocks` and `FocusState.strictUntil` in `Models.swift`; `AppData.recoverAfterLaunch` in `History.swift`; `setMode`, `modeChanged`, `strictLocked`, `startFocus`, `requestStartFocus`, `requestSkip`, `toggleTimer`, `skipPhase`, `stopTimer`, `apply`, `syncFocusFromTempo`, `switchGroup`, `startTask`, `importData` (idle only), `requestOverride`, `groupBinding`, `setAppBlocked`, the `rule.*` bridge messages and the launch in `init` in `apps/desktop/Sources/Kairos/AppModel.swift`; `ModePicker` (the one Mode control, used in the menu bar menu and on Today), the Quit buttons and `applicationShouldTerminate` in `KairosApp.swift`.

Watched but not modelled, because none of it changes the mode, the timer, the overrides or a group's rules: `apps/desktop/Sources/Kairos/KairosBar.swift`. Dragging a group along Kairos Bar writes `model.data.groups` directly (a reorder of the same groups), and dragging an app calls `addDockApp`, `removeDockApp` and `moveDockApp`, which only change which apps are pinned. It is in `check.sh`'s covered files so an edit there raises the `NOTE`: a write to `data.groups` in that file that changes a rule would skip `updateGroup` and the rule lock.

`requestStartFocus` and `requestSkip` ask before a strict focus phase starts. Agreeing is the model's `StartFocus` or `Skip`; cancelling changes nothing, so it is no step.

Last confirmed in sync: 2026-11-10, at commit 12c70d3 "Detect full screen from the window server's Space type", recorded as file checksums in `verification/strict-mode/synced.sha256`. Read between 6ce6322 and 12c70d3: the drags on Kairos Bar and `moveDockApp(_:to:)` (pinned apps and group order only), and the "Hide the apps that group blocks" switch option (`SwitchBehavior.hideBlocked`, `GroupSwitch.plan(only:)`, the hiding at the end of `switchGroup`). None changes what the model describes, so the model and the properties are unchanged. Write the commit hash here each time you run `--confirm`.

## What this does not cover

- It checks the model, not the Swift. `anchors.tsv` catches a mirrored line changing; it cannot catch new code that changes the mode some other way. That is what the `NOTE` is for.
- Bounds: `Max = 3` overrides. Short and long breaks are one phase. Grant expiry is "at some point", not five minutes.
- Quit and relaunch: a paused strict phase resumes running at launch, with its saved end (the time it would have ended if resumed at the last save); the model treats paused and running alike there.
- Rules are one bit per step: an edit or a switch is looser or it is not. Which edits count as looser is `RuleLock.loosens`, tested in `RuleLockTests`. The privacy list "Never send off this Mac" (Settings) also stops Jev from checking a site; on the Mac it is a privacy setting and stays editable in Strict. In Kairos for Linux, Settings changes go through `AppData::update_settings`, which refuses `rule_lock::settings_loosen` changes while locked: adding to that list, and turning off "Block browsers that Kairos Shield is not running in" or Jev's fail-closed switch. These are modelled as `EditRules`, as is pinning an app to the active group's Kairos Bar (Jev trusts those apps), which goes through `Model::update_group`.
- Hiding apps on a group switch (`SwitchBehavior`, `GroupSwitch.plan`) is left out. Returning to a group un-hides the apps hidden when it was left; an app that group blocks is hidden again by the app guard's sweep, which is not modelled.
- Left out: the Chrome extensions' own handling of the pushed state (they keep enforcing until `strictUntil` while Kairos is closed), app blocking and Jev, and the reason text on an override.

## Kairos for Linux

`check.sh --linux` checks the Rust port in `apps/linux` against the same model. Each step is one call on the GTK main thread in `apps/linux/crates/kairos/src/model.rs` (`Model` is `Rc` and every caller, including the tray and the bridge, reaches it through the main loop), so each is still one indivisible step.

How the model maps to the Rust:

| Model | Rust |
| --- | --- |
| `Override`, `LedgerRefusesStrict`, `LedgerChecksMax` | `OverrideLedger::request` in `kairos-core/src/rules.rs`, called from `Model::request_override` |
| `Ledger`, `ResetsOnEnterStrict` | `Model::mode_changed` (runs from `data_changed` on every mode change) and `TempoEvent::Started(Focus)` in `Model::apply` |
| `Synced`, `SyncsModeFromTimer` | `Model::sync_focus_from_tempo` |
| `SetMode`, `PickerLockedByTimer` | `Model::set_mode`, which returns early unless the timer is idle, from `mode_picker` on Today (`b.set_sensitive(idle)`) and the tray's Mode submenu (`enabled: !s.mode_locked`) |
| `StartFocus` | `Model::start_focus` (after `request_start_focus` asks, and rechecks idle) |
| `Skip`, `Stop`, `LocksStrictControls` | `Model::skip_phase` and `stop_timer` return early while `strict_locked()`; Skip and Stop are insensitive on Today and in the tray |
| `TimeUp`, `LongSleep`, `BreakOver` | `Model::tick` -> `TempoEngine::tick` / `advance`; `Stopped` and `AwaitingFocus` turn the mode off in `apply` |
| `SwitchGroup` | `Model::switch_group` (Kairos Bar, tray Group menu, group hotkeys, Groups page, Start or Make current on a task in another group), `delete_group`, `import_data` (idle only) |
| `EditRules`, `LocksRules` | `AppData::update_group` with `rule_lock::loosens`, reached from `Model::update_group` (Rules and Groups pages), `Model::set_app_blocked` (Kairos Bar) and the `rule.*` bridge messages |
| `Quit`, `QuitLocked` | `App::quit(false)` in `main.rs`, from the tray's Quit Kairos (disabled with a note while locked). Closing the window only hides it (`set_hide_on_close`); there is no other way Kairos quits itself |
| `ForceQuit` | SIGINT, SIGTERM, SIGHUP call `App::quit(true)`, which goes through (a logout cannot be refused); a crash or `kill -9` |
| `Quit` saving `strictUntil`, `Relaunch`, `ResumesStrict` | `focus.strict_until` in `sync_focus_from_tempo`; `AppData::recover_after_launch` (`history.rs`) and `tempo.resume_strict` in `Model::new` |

Covered (in `check.sh`'s Linux list, so an edit raises the `NOTE`): `kairos-core/src/rules.rs`, `tempo.rs`, `models.rs`, `history.rs`; `kairos/src/model.rs`, `main.rs`, `platform/tray.rs`, `ui/pages.rs`, `ui/window.rs`, `ui/bar.rs`, `ui/widgets.rs`. `bar.rs` and `widgets.rs` are watched, not modelled: they only call `switch_group`, `start_task`, `set_app_blocked`, `delete_group` and `update_group` for pinned apps. Reordering groups on the Groups page (`d.groups.swap` in `pages.rs`) writes `data.groups` directly but changes no rule.

Result on 2026-11-21: the same as the Mac. Six properties hold, every mutant fails, and `StrictKeepsGroup` fails with the same steps. In the code's words: launch with nothing saved (`recover_after_launch` sets the mode off); pick Strict on Today (`set_mode`); Start focus and agree (`start_focus`: `strict = s.mode() == Mode::Strict`, `tempo.start(TempoPhase::Focus, strict, ...)`); then click another group on Kairos Bar, pick it in the tray's Group menu, or press its group hotkey. `switch_group` (`model.rs`) only checks that the group is not already active, writes `d.focus = next`, and `data_changed` hides and pushes the new group's apps and sites (`bridge_state` reads `data.active_group()`), while the mode stays Strict. Deleting the active group does the same through `delete_group`.

Linux-only things the model does not show:

- Since 2026-11-21 `set_mode` refuses a change unless the timer is idle, as the views do, so a Mode press from a stale view (the tray menu is a snapshot sent over D-Bus and its command arrives later, `TrayCommand::SetMode(mode) => m.set_mode(mode)`; `MainWindow::rebuild` skips the rebuild while a text field or slider has focus) cannot take a strict phase out of Strict. On the Mac only the views lock the picker.
- Only one Kairos should run per data folder; two would keep two ledgers and two timers. The model assumes one.

Last confirmed in sync: 2026-11-21, at commit 86767ee "Add Kairos for Linux Mint, a Rust and GTK 4 port of Kairos Desktop", plus uncommitted edits by other work in `main.rs` (a single-instance lock) and `ui/window.rs` (rebuild on activation only when something outside Kairos changed); neither changes the mode, timer, overrides or rules. Recorded in `verification/strict-mode/synced-linux.sha256`.

## The Dock takeover has its own model

Kairos hides the macOS Dock and promises to put back exactly what was there. `verification/system-dock/SystemDock.tla` checks `SystemDock` and `DockWatchdog` (`apps/desktop/Sources/KairosCore/SystemDock.swift`) and the two Dock sinks and the quit paths in `KairosApp.swift`, over every order of launch, switching the replacement on and off, hiding the bar, quit, crash, lost preferences, the owner changing the Dock, and the watchdog's check and run as two steps.

| Property | In plain words | Harm if broken |
| --- | --- | --- |
| `NeverSavesOwn` | Kairos never records its own hidden setting as the owner's. | "Restore" leaves the Mac with no Dock for good. |
| `WayBack` | While the Dock is in Kairos's hidden setting, the restore script on disk holds a real setting. | A crash leaves nothing to put the Dock back with. |
| `WatchdogCovers` | A hidden Dock with Kairos stopped always has the watchdog installed. | The Dock stays hidden until Kairos is opened again. |
| `BackWhenNotReplacing` | With the bar hidden or the switch off while Kairos runs, the Dock is back. | No Dock and no Kairos Bar. |

```
verification/system-dock/check.sh --mutants   # about 15 seconds; needs strict-mode/check.sh to have downloaded TLC
```

All four hold. `--mutants` reruns them with each fix switched off (`RecoversFromScript`, `RefusesOwnValues`), and they must fail. A `NOTE` means `SystemDock.swift` or `KairosApp.swift` changed: read `replace()`, `restore()`, `recover()`, the watchdog and the sinks against the model, then run `check.sh --confirm`.

Found and fixed on 2026-11-10 (commit 4d36cb8). Both start with the saved values lost while the Dock is hidden (preferences reset, or never written before a crash):

- Crash, PrefsLost, Launch: `replace()` read the hidden Dock and recorded it as the owner's, overwriting the script too.
- PrefsLost, switch the replacement off: `restore()` did nothing, the watchdog was removed, and the Dock stayed hidden.

`replace()` and `restore()` now run a restore script left on disk first (`recover()`), and `replace()` never records `autohide-delay` 1000.

Last confirmed in sync: 2026-11-10, at commit 12c70d3, recorded in `verification/system-dock/synced.sha256`.

Not covered: the restore script being deleted while the Dock is hidden (Kairos then holds the only copy until it next runs `replace()`), a second display, the real timing of `defaults` and `killall Dock`, and whether `launchctl` loaded the watchdog.
