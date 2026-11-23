# Kairos

A native focus, distraction-blocking, and reading system for **macOS** (Swift / SwiftUI) and **Linux Mint** (Rust / GTK 4 on Cinnamon X11), paired with two Chromium extensions over a native-messaging Unix-socket bridge.

## Architecture and components

- **Kairos Desktop for macOS (`apps/desktop`).** Tasks, task groups, a Pomodoro timer with **Focus**, **Break**, and **Strict** modes, **Kairos Bar** (a context-switching dock that shows only the active group's apps and can replace the macOS Dock), native app blocking, and **Jev**, an AI relevance check that evaluates whether the front window or active browser tab fits your current task.
- **Kairos for Linux Mint (`apps/linux`).** A feature-complete Rust and GTK 4 implementation for Cinnamon on X11 (`kairos-core`, `kairos-host`, and `kairos`). It shares the macOS app's JSON data format (`kairos.json`), bridge protocol, and TLA+ specification. See [`apps/linux/README.md`](apps/linux/README.md) for Linux build details and Mac-to-Cinnamon mappings.
- **Kairos Shield (`extensions/shield`).** Enforces site block/allow rules, hides distracting page elements via CSS selectors, and runs Jev page relevance checks inside Chromium browsers. During a locked Strict session (`strictUntil`), Shield continues enforcing even if the desktop app is closed.
- **Kairos Read (`extensions/read`).** Extracts articles, narrates them sentence by sentence (using local system voices, OpenAI, or ElevenLabs with next-sentence prefetching), and generates interactive **Three.js 3D scenes** with Claude that highlight components in sync with the narrated sentence.
- **Native messaging protocol (`packages/shared/PROTOCOL.md`).** The bidirectional JSON contract between the extensions, the native messaging host (`com.kairos.host`), and the desktop app's Unix domain socket.

## Why Kairos

Most focus blockers fail at two boundaries:

1. **Static blocklists miss context, while blanket blocks break work.** A site like YouTube, GitHub, or Reddit can be either essential documentation or a distraction depending on the task. Kairos layers fast deterministic rules (domain block/allow lists, CSS element hiding, app blocklists) ahead of TypeSafe's **Jev** model, which checks the active page's title, URL, description, and excerpt against the task you are working on.
2. **State-machine edge cases let you escape "Strict" mode or leave your desktop broken.** In a focus app with a mode picker, a Pomodoro timer, temporary 5-minute overrides, per-group rule editors, quit handlers, and system Dock replacement, bugs hide in multi-step interleavings (for example: granting a 5-minute override in Focus mode and then switching to Strict, or crashing while the macOS Dock is hidden and losing saved preferences).

To prevent those bugs, Kairos formally specifies and model-checks its two most critical state machines in **TLA+** (`verification/strict-mode/` and `verification/system-dock/`), with every transition anchored line-by-line to both the Swift macOS code and the Rust Linux code.

## Formal verification (TLA+)

### 1. Strict mode state machine (`verification/strict-mode/StrictMode.tla`)

Checks every interleaving of mode changes, timer transitions, pause/resume, 5-minute override requests, rule edits, group switches, quitting, force-quitting, and relaunching against `anchors.tsv` (macOS Swift) and `anchors-linux.tsv` (Linux Rust):

| Invariant | Guarantee | Status |
|---|---|---|
| `StrictNoGrant` | While the mode is Strict, no temporary "Allow 5 min" override grant is ever active. | `PASS` |
| `StrictNoEscape` | No user action (including quitting, force-quitting, and relaunching) exits a locked strict focus phase before the clock runs out. | `PASS` |
| `QuitRefusedInStrict` | Ordinary Quit commands (menu bar, app menu, `Cmd+Q`, tray menu) are refused during a strict focus phase. | `PASS` |
| `UsedBounded` | At most `maxOverridesPerSession` overrides can be granted between two ledger resets. | `PASS` |
| `ModeMatchesTimer` | While a timer runs, the blocking mode pushed to the browser extensions matches the timer phase. | `PASS` |
| `StrictRulesHold` | No edit on the Rules page, Kairos Bar, or Kairos Shield can loosen the active group's blocking rules during a strict focus phase (adding stricter blocks remains allowed). | `PASS` |
| `StrictKeepsGroup` | Tracks whether switching to another task group during Strict can activate a looser rule set (documented design trade-off in `expected.tsv`). | Tracked |

### 2. System Dock and Cinnamon panel takeover (`verification/system-dock/`)

`SystemDock.tla` (macOS) and `CinnamonPanel.tla` (Linux Mint) verify that hiding the system Dock or Cinnamon panels across crashes, preference loss, concurrent watchdog firing, and manual System Settings changes never traps the user without a way back:

| Invariant | Guarantee | Status |
|---|---|---|
| `NeverSavesOwn` | Kairos never records its own hidden auto-hide setting as the user's original Dock or panel setting. | `PASS` |
| `WayBack` | Whenever the system Dock or panel is hidden by Kairos, the on-disk recovery script holds a valid restore configuration. | `PASS` |
| `WatchdogCovers` | Whenever the system Dock or panel is hidden and Kairos is not running, the LaunchAgent / `systemd` watchdog is armed. | `PASS` |
| `BackWhenNotReplacing` | Whenever Kairos is running with Kairos Bar hidden or Dock replacement turned off, the user's Dock or panel is restored. | `PASS` |
| `LeavesOwnersPanel` | (Linux) The recovery script never overwrites a panel configuration the user changed after restoration. | `PASS` |

Run both verification suites (including mutation tests that toggle off each guard constant and prove TLC finds a counterexample trace):

```sh
verification/strict-mode/check.sh --all --mutants   # checks macOS Swift + Linux Rust anchors and invariants
verification/system-dock/check.sh --all --mutants   # checks SystemDock.tla + CinnamonPanel.tla and mutants
```

Requires Java (`brew install openjdk` or `apt install openjdk-21-jre`); `strict-mode/check.sh` downloads a checksum-pinned `tla2tools.jar` on first run.

## Build and install

### macOS (`apps/desktop`, macOS 14+, Swift 5.10)

```sh
apps/desktop/scripts/build-app.sh
open apps/desktop/build/Kairos.app
```

To install into `/Applications/Kairos.app` and point all detected Chromium browsers' native messaging manifests (`com.kairos.host`) at the installed copy:

```sh
apps/desktop/scripts/build-app.sh --install
```

If a Developer ID certificate is present in your Keychain, the build script signs the app with it so Keychain and Accessibility permissions persist across rebuilds; otherwise it signs ad hoc.

### Linux Mint (`apps/linux`, Cinnamon on X11, Rust + GTK 4)

```sh
apps/linux/scripts/install.sh
```

Installs any missing GTK 4, GStreamer, and D-Bus development packages, compiles a release binary into `~/.local/lib/kairos`, links `~/.local/bin/kairos`, registers the desktop entry, and writes native messaging manifests for Chrome, Chromium, Brave, Edge, and Vivaldi. See [`apps/linux/README.md`](apps/linux/README.md) for full details.

### Load the browser extensions

The extensions run in Chromium browsers (Chrome, Brave, Arc, Edge, Vivaldi):

1. Open `chrome://extensions` (or `brave://extensions`, `edge://extensions`).
2. Enable **Developer mode**.
3. Click **Load unpacked** and select `extensions/shield`, then repeat for `extensions/read` (or use **Settings > Chrome bridge > Show extensions in Finder**).

When Kairos starts, it registers the native messaging host (`com.kairos.host`) automatically; **Settings > Chrome bridge** shows live connection status for both extensions.

## API keys, 3D scenes, and privacy

All API keys are optional and stored in your login Keychain (macOS) or Secret Service keyring (Linux) under **Settings > API keys**:

| Integration | Used for | Fallback without an API key |
|---|---|---|
| **TypeSafe** | Jev on-screen relevance checks (`https://docs.typesafe.ai`) | Deterministic site and app block/allow lists still apply |
| **Claude / Anthropic** | Interactive Three.js 3D scenes in Kairos Read | Uses your local **Claude Code subscription** (`claude` CLI + Node.js via `scene-agent.mjs`) with no API key |
| **ElevenLabs / OpenAI** | Cloud narration voices in Kairos Read | Local system voices (free) |

**Privacy guard (`neverAnalyse`):** Any hostname listed under **Settings > Privacy > Never send off this Mac** is excluded from Jev checks, cloud TTS narration, and 3D scene generation. Both the extensions and the desktop host enforce this list independently.

## Permissions and shortcuts

- **Accessibility (macOS):** Lets Jev read the front window's title and lets Kairos switch desktop Spaces. Prompted from **Settings > Blocking > Read window titles**.
- **Notifications:** Alerts you when a focus session or break ends and when tasks come due (scheduling the soonest 64 notifications within macOS's per-app limit and rolling later ones in as earlier ones fire).
- **Use Kairos Bar instead of the macOS Dock / Cinnamon panel:** Hides the system Dock or panel while Kairos Bar is active and arms `com.kairos.dock-watchdog` (macOS LaunchAgent) or a `systemd` user timer (Linux) to restore it within seconds if Kairos terminates unexpectedly.
- **Global shortcuts:** `Option+Space` adds a task from anywhere; `Option+Shift+Space` starts or pauses the timer; `Control+1` through `Control+9` can switch task groups (configurable under **Settings > General**).

## Testing

```sh
# macOS app logic, native-messaging bridge end-to-end check, and UI snapshot renderer
cd apps/desktop
swift test
python3 scripts/bridge-check.py
scripts/snapshot.sh /tmp/kairos

# Linux core and native host (runs on both macOS and Linux)
(cd apps/linux && cargo test)

# Chromium extensions
(cd extensions/shield && node --test)
(cd extensions/read && node --test)
```

`bridge-check.py` and `snapshot.sh` run against an isolated `KAIROS_HOME` scratch profile and a separate bundle ID (`scripts/scratch-copy.sh`), so they never touch your live `~/Library/Application Support/Kairos/kairos.json` data, browser manifests, Keychain, Dock, or Notification Center.
