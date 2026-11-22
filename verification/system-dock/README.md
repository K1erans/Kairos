# System dock takeover models

Kairos hides the system dock while Kairos Bar stands in for it, and promises to put back exactly
what was there. There are two models, one per platform:

| Model | Code it mirrors | Run |
| --- | --- | --- |
| `SystemDock.tla` | macOS: `apps/desktop/Sources/KairosCore/SystemDock.swift`, the Dock sinks and quit paths in `apps/desktop/Sources/Kairos/KairosApp.swift` | `check.sh [--mutants]` |
| `CinnamonPanel.tla` | Linux Mint: `apps/linux/crates/kairos-core/src/panel.rs`, `sync_panel()`, `finish()`, the signal handlers and `--restore-panel` in `apps/linux/crates/kairos/src/main.rs` | `check.sh --linux [--mutants]` |

```
verification/system-dock/check.sh --mutants           # Mac, as before (about 10 s)
verification/system-dock/check.sh --linux --mutants   # Linux (about 25 s)
verification/system-dock/check.sh --all --mutants     # both
verification/system-dock/check.sh --confirm --linux   # after reading the Rust against the model
```

TLC comes from `verification/strict-mode/.tools/tla2tools.jar`: run `verification/strict-mode/check.sh` once to download it.
With no target flag, `check.sh` checks only the Mac model and prints exactly what it always has.
`--confirm` on its own still records only the Mac files (`synced.sha256`). `--confirm --linux` records the Rust files (`synced-linux.sha256`), and `--confirm --all` records both.
A `NOTE` means a covered file changed since it was last confirmed.

The Mac section of `.claude/skills/verify-strict-mode/SKILL.md` describes `SystemDock.tla`. This file covers the Linux model.

## Why Linux has its own model

The two platforms use the same idea: save the owner's setting, apply Kairos's own, leave a restore script on disk, and have a timer run it if Kairos stops. The Linux code puts the pieces together differently, so `SystemDock.tla` with renamed values would check something the Rust code does not do:

| | Mac (`SystemDock.tla`) | Linux (`CinnamonPanel.tla`) |
| --- | --- | --- |
| The setting | `com.apple.dock` `autohide`, `autohide-delay` 1000, `static-only` | `org.cinnamon` `panels-autohide` (every panel `:true`), `panels-show-delay` 2000 |
| Saved values | `systemDockSaved` in UserDefaults | `panel-saved.json` in the data folder |
| Restore script | `dock-restore.sh`. It removes itself and the saved values. | `restore-panel.sh`. It removes `panel-saved.json` and **stays on disk**. It begins with its own `pgrep -u "$(id -u)" -x kairos` check (this user's Kairos only). |
| Watchdog | A LaunchAgent, every 15 s. It is installed while the switch is on, even with the bar hidden. | The `kairos-panel-watchdog` systemd user timer, every 30 s. `replace()` installs it and `restore()` removes it. |
| When Kairos acts | Two Combine sinks fire on a change to `hidesSystemDock` / `replacesSystemDock` | `sync_panel()` runs at launch and on **every** DATA change. It does nothing when the wanted state matches both `panel_hidden` (in memory) and `is_replaced()` (the file exists). |
| `replace()` with values already saved | Rewrites the script and leaves the Dock as it is | Rewrites the script and hides the panel again |
| `restore()` with nothing saved | Runs a leftover script (`recover()`) | Does nothing, then deletes the script and the timer |
| Quit | `restore()` always | `finish()` calls `restore()` only if `panel_hidden` is set |
| Extra way back | None | `kairos --restore-panel` runs `restore()` from a second process |

### Actions

| `SystemDock.tla` | `CinnamonPanel.tla` | Rust |
| --- | --- | --- |
| `Launch` | `Launch` (`panel_hidden` starts false) | `start()` → `sync_panel()` |
| `ToggleReplaces` | `ToggleReplaces` | Settings switch → `set_replaces_system_dock` → DATA → `sync_panel()` |
| `ToggleVisible` | `ToggleVisible` | "Show Kairos Bar" in Settings or the tray → DATA → `sync_panel()` |
| (none) | `DataChanged` | Any other edit → DATA → `sync_panel()` with the same settings |
| `Quit` | `Quit` | Tray Quit or SIGINT/SIGTERM/SIGHUP → `app.quit()` → `finish()` |
| `Crash` | `Crash` | SIGKILL, crash, power loss |
| (none) | `RestoreCommand` | `kairos --restore-panel`, by hand or from `install.sh --uninstall` |
| `PrefsLost` | `PrefsLost` | `panel-saved.json` deleted, or unreadable so that `load_saved()` returns `None` |
| `OwnerSetsDock` | `OwnerSetsDock` | The owner changes auto-hide in Cinnamon's panel settings |
| `WatchdogChecks` | `WatchdogChecks` | The timer fires and the script's `pgrep` finds no `kairos` |
| `WatchdogRuns` | `WatchdogRuns` + `WatchdogRemoves` | The script's two `gsettings set` calls, then its `rm panel-saved.json`. A Kairos that starts in between does not stop it. |

### Properties

| Property | In plain words, on Linux | Harm if broken |
| --- | --- | --- |
| `NeverSavesOwn` | Kairos never records its own auto-hide-everything setting as the owner's. | "Restore" leaves every panel hidden for good. |
| `WayBack` | While the panels are in Kairos's setting, `restore-panel.sh` holds a real setting. | A crash leaves nothing that can bring the panel back. |
| `WatchdogCovers` | A hidden panel with Kairos stopped always has the timer enabled. | The panel stays hidden until Kairos is opened again. |
| `BackWhenNotReplacing` | With Kairos Bar hidden, or the switch off, while Kairos runs, the panel is back. | No panel and no Kairos Bar. |
| `LeavesOwnersPanel` (Linux only) | Once the panel has been put back, Kairos does not overwrite changes the owner makes later. | The owner's panel settings keep reverting every 30 s. |

### Fix switches

There are four switches. The Rust code has had all four since 2026-11-21 (`recover()`, `is_own_setting()`, the script's `rm -f "$0"` and `panel.lock` in `panel.rs`), so the `code` run sets all of them to TRUE.

| Switch | What the code would do |
| --- | --- |
| `RecoversFromScript` | With nothing saved, `replace()` and `restore()` first run a leftover `restore-panel.sh --force`, as the Mac `recover()` does. |
| `RefusesOwnValues` | `replace()` never records Kairos's own values (every panel `:true`, show delay 2000). With nothing better known, it records the schema default (`gsettings reset`). |
| `ScriptRemovesItself` | `restore-panel.sh` ends with `rm -f "$0"`, as the Mac script does. |
| `ScriptLocked` | The script holds a lock from before its `pgrep` until after its `rm`, and `replace()` and `restore()` take the same lock (for example `flock` on a file in the data folder). A Kairos that starts mid-script then waits for the script to finish. |

`--mutants` for Linux also runs the code as it was before the fixes (all five fail), then the code with each switch turned off on its own. All five properties hold on the code as it is. Turning off `RecoversFromScript` or `ScriptLocked` breaks `WayBack`, `WatchdogCovers` and `BackWhenNotReplacing`. Turning off `ScriptRemovesItself` breaks `LeavesOwnersPanel`. Turning off `RefusesOwnValues` breaks nothing in this model: it is a backstop for the case where `restore-panel.sh` is deleted while the panel is hidden, and the model leaves that case out, as the Mac model does.

## Results on the Rust code

All five properties hold since the fixes of 2026-11-21. The script also stops the timer (`systemctl --user disable --now`) after it puts the panel back; it does that while holding `panel.lock`, so a Kairos that starts meanwhile re-enables the timer in `replace()` after it. The rest of this section records what the check found before the fixes.

All five properties failed on the code before the fixes. Each failure was walked through the Rust code, and all are real bugs, not modelling errors. Every one of them, except the last, needs `panel-saved.json` to be gone while the panel is hidden. Kairos does that to itself:

**How `panel-saved.json` goes missing with no one deleting it.** TLC finds this trace even with `PrefsLost` removed from the model:

1. With the switch on, Kairos hides the panel. `replace()` saves A to `panel-saved.json`, writes `restore-panel.sh`, enables the timer and sets `panels-autohide` to all `:true`. Then Kairos crashes, is killed, or the machine loses power.
2. The timer fires. `restore-panel.sh` passes its `pgrep -x kairos` check (panel.rs:91) and runs its two `gsettings set` calls.
3. Kairos starts at that moment. At login after a crash, autostart and the timer run close together. `sync_panel()` (main.rs:311) calls `panel::replace()`. `load_saved()` still finds A, so it rewrites the script, enables the timer and hides the panel again. `panel_hidden` is now true.
4. The script runs its last line, `rm -f panel-saved.json` (panel.rs:94).

Kairos is now running with the panel hidden and nothing saved. Deleting the file by hand, or a file `load_saved()` can't parse (panel.rs:196-199), gives the same state.

**1. `NeverSavesOwn`: Kairos saves its own hidden setting as the owner's.** From that state, any edit emits DATA and `sync_panel()` runs. `want` is true and `panel_hidden` is true, but `is_replaced()` is now false (main.rs:316), so it calls `replace()` again. `load_saved()` returns `None`, so it reads the current `panels-autohide` and `panels-show-delay` (panel.rs:163-164), which are Kairos's own values, and saves them to both `panel-saved.json` and `restore-panel.sh`. From then on, every restore, by `restore()`, by the timer or by `--restore-panel`, "puts back" auto-hide with a 2000 ms delay. The owner's real setting is gone.

**2. `WayBack` and `BackWhenNotReplacing`: switching off leaves the panel hidden with no way back.** From the same state, the owner turns off "Use Kairos Bar instead of Cinnamon’s panel", or hides Kairos Bar. `sync_panel()` calls `restore()` (main.rs:325). `load_saved()` returns `None`, so nothing is put back (panel.rs:184). Then `remove_watchdog()` (panel.rs:189, 213-220) deletes `restore-panel.sh` and the timer anyway. The panel stays auto-hidden, and nothing on disk can bring it back. `kairos --restore-panel` does nothing either.

**3. `WatchdogCovers`: quitting leaves the panel hidden with no timer.** From the same state, the owner quits. `finish()` (main.rs:431-433) sees `panel_hidden` and calls `restore()`, which again puts nothing back and removes the timer. Kairos has stopped, the panel is hidden, and no timer is left to restore it.

**4. `LeavesOwnersPanel`: after a crash, the timer keeps reverting the owner's panel.** This one needs no lost file:

1. Kairos crashes while hiding the panel.
2. Within 30 s, `restore-panel.sh` puts A back and removes `panel-saved.json`. The script and the timer stay. The script has no `rm "$0"` (panel.rs:88-99), and only `remove_watchdog()` deletes them, which runs only from `restore()`.
3. The owner changes their panel, say turns auto-hide on (B).
4. The next timer run, within 30 s, sets A again. This repeats every 30 s and at every login until Kairos next runs `restore()`. If Kairos is next started with no X11 display, or with the panel not wanted, `sync_panel()` returns early (main.rs:316) and the timer stays.

These are the two bugs fixed on the Mac on 2026-11-10 (commit 4d36cb8), and the Rust port does not have either fix. The script race in steps 2-4 above, and the script that stays after running, are Linux-only. A plain port of the Mac fix is not enough: with `RecoversFromScript`, `RefusesOwnValues` and `ScriptRemovesItself` but no `ScriptLocked`, the same race still deletes the newly written script along with `panel-saved.json`. That is the `no-lock` run.

Last confirmed in sync: 2026-11-21, recorded in `synced-linux.sha256`. That `main.rs` includes another agent's uncommitted single-instance lock (`kairos.lock`), which makes "one Kairos at a time" true and does not touch the panel code. Both files were read side by side with the model.

`panel.rs` re-read and its checksum alone updated on 2026-11-21 after two changes to the script: every saved value and path is single-quoted (a `$`, backtick or `\` in a gsettings value is no longer run or expanded by sh), and the check is `pgrep -u "$(id -u)" -x kairos`, so another user's Kairos no longer stops it. The script's steps and their order (lock, check, two `gsettings set`, `rm` of the saved values, stop the timer, `rm "$0"`) are unchanged, so the model is too; all five properties hold and every mutant still fails. A scratch copy (`KAIROS_HOME` set) run by the same user still counts as running. `main.rs` was left out of that update because another agent is editing it.

## Not covered

- `restore-panel.sh` deleted while the panel is hidden (the same gap as the Mac model).
- `replace()` failing partway, for example `gsettings` refusing `panels-autohide` after the file and the timer are written. The failure leaves `panel_hidden` false, so `finish()` will not restore.
- Panels added or removed while hidden: the saved arrays list only the panels that existed when they were saved.
- A running `sh` keeps reading the script file it opened, so it uses the old file even after Kairos rewrites `restore-panel.sh`. The model uses the current text.
- Whether `systemctl --user enable --now` actually started the timer, and the timer's real timing.
- How `Settings::hides_system_dock()` (kairos-core/src/models.rs) is derived: it is modelled as `replaces /\ visible`. `models.rs` is not in the checksum, because it changes often for unrelated reasons.

The Mac model treats the Mac script as one step, and it was left unchanged. The same mid-script start can happen on the Mac. I reasoned it through but did not model it: it looks harmless there, because the Mac `replace()` with values saved does not hide the Dock again.
