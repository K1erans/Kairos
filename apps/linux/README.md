# Kairos for Linux Mint

Kairos Desktop for Linux Mint with Cinnamon on X11, written in Rust and GTK 4. It has the Mac
app's features: tasks, groups, the Pomodoro timer with Focus, Break and Strict, app blocking, the
bridge to Kairos Shield and Kairos Read, Jev, 3D scenes, voices, Kairos Bar, and hiding other
groups' apps. Its data file has the same format as the Mac app's, so Settings > Data moves
everything between the two.

## Install

```sh
apps/linux/scripts/install.sh
```

The script installs the GTK 4, GStreamer and D-Bus build packages if they are missing (it asks
for your password), builds a release, and installs Kairos for your user:

- the app goes in `~/.local/lib/kairos`
- `kairos` is linked into `~/.local/bin`
- the app is added to the menu
- Brave, Chrome, Chromium, Edge and Vivaldi are pointed at its browser host

Run the script again to update. `install.sh --uninstall` removes the app and puts the Cinnamon
panel back. Your data stays in `~/.local/share/kairos`.

You need Rust (https://rustup.rs). For 3D scenes on a Claude subscription you also need `npm`
before you install; without it, scenes need an Anthropic API key. Load the extensions as the
main README describes, from `~/.local/lib/kairos/resources/extensions`. Settings > Chrome bridge
has a button that opens that folder.

## How it maps to the Mac app

| Mac | Linux Mint |
| --- | --- |
| Menu bar item | Tray icon in Cinnamon's panel (StatusNotifierItem) |
| Hiding an app | Minimising all of its windows |
| Spaces | Workspaces (Groups > Switch to workspace) |
| Replacing the Dock | Setting Cinnamon's panels to auto-hide. A systemd user timer restores them within 30 s if Kairos stops without doing it. |
| Keychain | Login keyring (Secret Service) |
| Second push to reveal Kairos Bar over a full-screen app | Rest the pointer on the edge for a moment |
| Notification Center | Desktop notifications |

Kairos Bar is a dock window. It reserves its edge while pinned, so maximised windows stop at it.
Drag an app off Kairos Bar to remove it, and drag the divider to resize it. Drop `.desktop`
files on it to add apps, and drop documents on an app to open them.

Global shortcuts are grabbed from X11. Settings says when another app already holds one: on Mint,
Ctrl+Alt+Space is often taken, so try Ctrl+Alt+N for quick add.

## Develop

```sh
cd apps/linux
cargo test                  # kairos-core and kairos-host; runs on macOS too
cargo build -p kairos       # the GTK app; needs Linux with the packages above
KAIROS_HOME=/tmp/k ./target/debug/kairos
```

With `KAIROS_HOME` set, Kairos uses that folder for its data and socket. It also leaves your
panel, keyring, autostart and browser manifests alone, and doesn't block apps.

`KAIROS_PAGE=Settings` opens a page at launch, and `KAIROS_START_FOCUS=1` starts a focus session.
Kairos uses GTK's cairo renderer to save memory. Set `GSK_RENDERER` to use another.

The code is split into three crates:

- `crates/kairos-core`: the logic, with no GUI. It holds the models, rules, timer, Kairos Bar
  geometry, bridge, Jev, voices, scenes and Cinnamon panel settings.
- `crates/kairos-host`: the native messaging host that Chromium starts.
- `crates/kairos`: the GTK app and its X11 window handling.
