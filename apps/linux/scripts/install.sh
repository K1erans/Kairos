#!/usr/bin/env bash
# Builds Kairos for Linux and installs it for the current user:
#   ~/.local/lib/kairos       the app, the browser host and resources (extensions, scene helper)
#   ~/.local/bin/kairos       a link to the app
#   ~/.local/share/applications/kairos.desktop and the icon
# Then points Chromium browsers at the host so Kairos Shield and Kairos Read can connect.
# Run it again to update. scripts/install.sh --uninstall removes it.
set -euo pipefail

HERE="$(cd "$(dirname "$0")/.." && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
LIB="$HOME/.local/lib/kairos"
BIN="$HOME/.local/bin"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor/scalable/apps"
CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}"

# The PIDs of this user's running installed Kairos ($LIB/kairos), not dev or test copies.
installed_pids() {
  local want pid exe
  want="$(realpath -m "$LIB/kairos")"
  for pid in $(pgrep -u "$(id -u)" -x kairos || true); do
    exe="$(readlink "/proc/$pid/exe" 2>/dev/null || true)"
    if [[ "$exe" == "$want" || "$exe" == "$want (deleted)" ]]; then echo "$pid"; fi
  done
}

# Asks the installed Kairos to quit and waits for it to exit: on the way out it puts Cinnamon's
# panel back, which can take a few seconds, and a copy started before then would find the
# instance lock held and exit. Returns 1 when none was running.
stop_installed() {
  local pids
  pids="$(installed_pids)"
  [[ -n "$pids" ]] || return 1
  # shellcheck disable=SC2086
  kill $pids 2>/dev/null || true
  for _ in $(seq 1 150); do
    [[ -z "$(installed_pids)" ]] && return 0
    sleep 0.1
  done
  echo "warning: Kairos did not quit within 15 seconds" >&2
  return 0
}

if [[ "${1:-}" == "--uninstall" ]]; then
  stop_installed || true
  if [[ -x "$LIB/kairos" ]]; then
    "$LIB/kairos" --restore-panel 2>/dev/null || true
  fi
  # The same browser list and config folder the app installs to. A Kairos older than
  # --uninstall-bridge would start its window on an unknown flag, so look for it first.
  if [[ -x "$LIB/kairos" ]] && grep -qa -- "--uninstall-bridge" "$LIB/kairos"; then
    "$LIB/kairos" --uninstall-bridge 2>/dev/null || true
  else
    for d in google-chrome google-chrome-beta google-chrome-unstable chromium BraveSoftware/Brave-Browser microsoft-edge microsoft-edge-beta vivaldi opera; do
      rm -f "$CONFIG/$d/NativeMessagingHosts/com.kairos.host.json"
    done
  fi
  rm -rf "$LIB" "$BIN/kairos" "$APPS/kairos.desktop" "$ICONS/kairos.svg" "$CONFIG/autostart/kairos.desktop"
  echo "Kairos removed. Your data is still in ${XDG_DATA_HOME:-$HOME/.local/share}/kairos."
  exit 0
fi

need=(libgtk-4-dev libgstreamer1.0-dev libdbus-1-dev pkg-config gstreamer1.0-plugins-good)
missing=()
for p in "${need[@]}"; do dpkg -s "$p" >/dev/null 2>&1 || missing+=("$p"); done
if (( ${#missing[@]} )); then
  echo "Installing build packages: ${missing[*]}"
  sudo apt-get install -y "${missing[@]}"
fi
command -v cargo >/dev/null || { echo "Rust is needed: https://rustup.rs" >&2; exit 1; }

(cd "$HERE" && cargo build --release -p kairos -p kairos-host)

running=0
if stop_installed; then running=1; fi

mkdir -p "$LIB/resources/extensions" "$BIN" "$APPS" "$ICONS"
install -m 755 "$HERE/target/release/kairos" "$LIB/kairos"
install -m 755 "$HERE/target/release/kairos-host" "$LIB/kairos-host"
rm -rf "$LIB/resources/extensions/shield" "$LIB/resources/extensions/read"
cp -R "$REPO/extensions/shield" "$REPO/extensions/read" "$LIB/resources/extensions/"
install -m 644 "$HERE/resources/kairos.svg" "$ICONS/kairos.svg"

# The scene helper for 3D scenes on a Claude subscription.
HELPER="$LIB/resources/scene-agent"
mkdir -p "$HELPER"
cp "$REPO/apps/desktop/scene-agent/scene-agent.mjs" "$REPO/apps/desktop/scene-agent/package.json" "$REPO/apps/desktop/scene-agent/package-lock.json" "$HELPER/"
if command -v npm >/dev/null; then
  (cd "$HELPER" && npm ci --omit=dev --no-audit --no-fund --loglevel=error) || echo "warning: npm ci failed; scenes need an Anthropic API key" >&2
else
  echo "warning: npm not found, so there is no scene helper; scenes need an Anthropic API key" >&2
fi

ln -sf "$LIB/kairos" "$BIN/kairos"
cat > "$APPS/kairos.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Kairos
Comment=Focus, tasks and app groups
Exec="$LIB/kairos"
Icon=kairos
Categories=Utility;Office;
StartupWMClass=kairos
Keywords=focus;pomodoro;tasks;block;
DESKTOP
update-desktop-database "$APPS" 2>/dev/null || true
gtk-update-icon-cache -q "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

"$LIB/kairos" --install-bridge

echo "Kairos is installed. Open it from the menu, or run: kairos"
if (( running )); then nohup "$LIB/kairos" >/dev/null 2>&1 & fi
