//! Small links to the desktop: API keys in the login keyring, start at login, where Kairos's
//! own files are installed, and the browser bridge manifests.

use kairos_core::keys::{KeyStore, ScratchKeys};
use kairos_core::store;
use std::path::PathBuf;

pub const KEY_SERVICE: &str = "com.kairos.desktop";

pub mod keys {
    use super::*;

    pub fn get(account: &str) -> Option<String> {
        match KeyStore::current() {
            KeyStore::File(path) => ScratchKeys { path }.get(account),
            KeyStore::Keyring => keyring::Entry::new(KEY_SERVICE, account).ok()?.get_password().ok().filter(|s| !s.is_empty()),
        }
    }

    pub fn set(value: &str, account: &str) -> bool {
        match KeyStore::current() {
            KeyStore::File(path) => ScratchKeys { path }.set(value, account),
            KeyStore::Keyring => {
                let Ok(entry) = keyring::Entry::new(KEY_SERVICE, account) else { return false };
                if value.is_empty() {
                    let _ = entry.delete_credential();
                    true
                } else {
                    entry.set_password(value).is_ok()
                }
            }
        }
    }
}

/// Where the installer put the binaries and resources: beside this executable.
pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.canonicalize().ok()).and_then(|p| p.parent().map(PathBuf::from)).unwrap_or_default()
}

pub fn host_path() -> PathBuf {
    exe_dir().join("kairos-host")
}

/// The installed resources folder (extensions, scene helper, icon), when there is one.
pub fn resources() -> Option<PathBuf> {
    let dir = exe_dir().join("resources");
    dir.exists().then_some(dir)
}

pub fn extensions_folder() -> Option<PathBuf> {
    let dir = resources()?.join("extensions");
    dir.exists().then_some(dir)
}

pub mod bridge_installer {
    use super::*;

    /// Points every Chromium browser's native messaging manifest at this copy's host. Never
    /// from a scratch copy.
    pub fn install() -> Vec<String> {
        if store::is_scratch() || !host_path().exists() {
            return vec![];
        }
        kairos_core::bridge::install_manifests(&host_path())
    }

    pub fn installed_browsers() -> Vec<String> {
        kairos_core::bridge::installed_browsers(&host_path())
    }

    /// Removes the native messaging manifest from every browser `install` writes to (the same
    /// list and config folder), when it points at this copy's host or at a host that is gone.
    /// Never from a scratch copy. Returns the browsers it was removed from.
    pub fn uninstall() -> Vec<String> {
        if store::is_scratch() {
            return vec![];
        }
        let host = host_path();
        let config = store::config_dir();
        let mut done = vec![];
        for (name, dir) in kairos_core::bridge::BROWSER_DIRS {
            let file = config.join(dir).join(format!("{}.json", kairos_core::bridge::HOST_NAME));
            let Ok(data) = std::fs::read(&file) else { continue };
            let points_at = serde_json::from_slice::<serde_json::Value>(&data)
                .ok()
                .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(PathBuf::from));
            let ours = match points_at {
                Some(p) => p == host || !p.exists(),
                None => true, // Unreadable: not a manifest anything can use.
            };
            if ours && std::fs::remove_file(&file).is_ok() {
                done.push(name.to_string());
            }
        }
        done
    }
}

pub mod autostart {
    use super::*;

    fn path() -> PathBuf {
        store::config_dir().join("autostart/kairos.desktop")
    }

    pub fn enabled() -> bool {
        std::fs::read_to_string(path()).map(|s| !s.contains("Hidden=true") && !s.contains("X-GNOME-Autostart-enabled=false")).unwrap_or(false)
    }

    /// An entry written by an older Kairos opened the main window at login; this one starts in
    /// the background. Rewrites an old entry that is on.
    pub fn upgrade() {
        if store::is_scratch() || !enabled() {
            return;
        }
        let current = std::fs::read_to_string(path()).unwrap_or_default();
        // Only an entry that starts this copy, so a test build never takes over login.
        let exe = format!("Exec=\"{}\"", exe_dir().join("kairos").display());
        if current.contains(&exe) && !current.contains("--background") {
            set(true);
        }
    }

    pub fn set(on: bool) {
        if store::is_scratch() {
            return;
        }
        if on {
            // --background: at login Kairos starts in the tray and Kairos Bar, without its window.
            let exe = exe_dir().join("kairos");
            let entry = format!(
                "[Desktop Entry]\nType=Application\nName=Kairos\nComment=Focus, tasks and app groups\nExec=\"{}\" --background\nIcon=kairos\nX-GNOME-Autostart-enabled=true\nNoDisplay=false\n",
                exe.display()
            );
            let _ = std::fs::create_dir_all(path().parent().unwrap());
            let _ = store::write_atomic(&path(), entry.as_bytes());
        } else {
            let _ = std::fs::remove_file(path());
        }
    }
}

/// Shows a folder in the file manager.
pub fn show_folder(path: &std::path::Path) {
    use gtk::gio;
    use gtk::prelude::*;
    let ctx = gtk::gdk::Display::default().map(|d| d.app_launch_context());
    let _ = gio::AppInfo::launch_default_for_uri(&gio::File::for_path(path).uri(), ctx.as_ref());
}
