# Kairos

A Mac focus app with two Chrome extensions.

- **Kairos Desktop** (`apps/desktop`): tasks, groups, a Pomodoro timer with Focus, Break and
  Strict modes, Kairos Bar (a dock for the active group), app blocking, and Jev, an AI check that
  asks whether what is on screen fits the task you are working on.
- **Kairos Shield** (`extensions/shield`): blocks sites and hides page elements in the browser
  while you focus. Without it Kairos cannot block any site.
- **Kairos Read** (`extensions/read`): reads articles aloud and builds 3D scenes from them.
- `packages/shared/PROTOCOL.md` is the message contract between the app and the extensions.

Kairos Desktop needs macOS 14 or later. The extensions run in Chromium browsers (Chrome, Brave,
Arc, Edge, Vivaldi); Safari and Firefox cannot run them.

## Build

```sh
apps/desktop/scripts/build-app.sh
open apps/desktop/build/Kairos.app
```

This needs Xcode or the Command Line Tools (Swift 5.10). With a Developer ID certificate in your
keychain the app is signed with it, so Keychain and Accessibility permissions stay granted across
rebuilds; without one it is signed ad hoc and macOS may ask again after a rebuild.

## Install

```sh
apps/desktop/scripts/build-app.sh --install
```

This builds, copies the app to `/Applications/Kairos.app` (the copy in `apps/desktop/build` stays),
points the browsers' native messaging manifests at the installed copy, and prints the steps to
switch over: quit the running Kairos (not possible during a strict session until it ends), open
`/Applications/Kairos.app`, and turn "Open Kairos at login" off and on again if you use it.

## Load the extensions

1. Open `chrome://extensions` (Brave: `brave://extensions`, Edge: `edge://extensions`).
2. Turn on Developer mode.
3. Choose "Load unpacked" and pick `extensions/shield`, then again for `extensions/read`.
   Settings > Chrome bridge > "Show extensions in Finder" opens the copies inside the app.

When it starts and no browser points at it yet, Kairos Desktop writes the native messaging manifest
(`com.kairos.host`) for every Chromium browser it finds; Settings > Chrome bridge > "Install bridge"
does it on demand. Settings
shows which extensions are connected. While blocking is on and no Shield is connected, Today, the
menu bar menu and Kairos Bar say that site blocking is off.

## Keys

All optional. Paste them in Settings > API keys; they are stored in your login Keychain.

| Key | For | Without it |
| --- | --- | --- |
| TypeSafe | Jev (https://docs.typesafe.ai) | Only your block and allow lists apply |
| Anthropic | 3D scenes in Kairos Read | No scenes |
| ElevenLabs or OpenAI | Cloud voices in Kairos Read (Settings > Reading voice) | Mac voices, which are free |

Sites on Settings > Privacy > "Never send off this Mac" are never sent to Jev, a cloud voice or
Claude.

## Permissions

- **Accessibility**: lets Jev read the front window's title, and lets Kairos switch desktops with
  Control-number. Settings > Blocking > "Read window titles" asks for it.
- **Notifications**: for the end of a focus session or break. When a break ends the timer waits
  for you to start the next focus session, unless Settings > Timer > "Start the next focus session
  automatically" is on.
- **Open at login**: Settings > General.
- **Use Kairos Bar instead of the macOS Dock** (Settings > Kairos Bar) hides the Dock with its own
  auto-hide settings and puts them back when turned off or when Kairos quits. While it is on, a
  small LaunchAgent (`com.kairos.dock-watchdog`) brings the Dock back if Kairos stops without doing
  it, and is removed when the switch is turned off.

Press Option-Space anywhere to add a task, and Option-Shift-Space to start or pause the timer.
Settings > General changes or turns off both shortcuts. Control-1 to Control-9 can switch groups;
that is off for new profiles because macOS can use those keys to switch desktops.

## Tests

```sh
cd apps/desktop
swift test                          # the app's logic
python3 scripts/bridge-check.py     # drives a built app through its native host, Chrome's way
scripts/snapshot.sh /tmp/kairos     # renders every screen and Kairos Bar state to PNG

(cd extensions/shield && node --test)
(cd extensions/read && node --test)
```

`bridge-check.py` and `snapshot.sh` run Kairos against a scratch profile (`KAIROS_HOME`), so they
never touch your data, your browser manifests or the Dock. `bridge-check.py` works from any
directory and checks `apps/desktop/build/Kairos.app` unless given another path.

Kairos keeps its data in `~/Library/Application Support/Kairos/kairos.json`. Set `KAIROS_HOME` to
another folder to run a separate copy with its own data and socket.

## Verification

Strict mode (no overrides, no stopping, no quitting until the session ends) is described as a
TLA+ model in `verification/strict-mode`, with each step tied to a line of code.

```sh
verification/strict-mode/check.sh            # anchors, then every property
verification/strict-mode/check.sh --mutants  # also shows each property fails on a broken variant
```

It needs Java (`brew install openjdk`) and downloads TLC once. When you change the mode, the timer,
overrides or quitting, follow `.claude/skills/verify-strict-mode/SKILL.md`.
