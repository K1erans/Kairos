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
}

pub mod autostart {
    use super::*;

    fn path() -> PathBuf {
        store::config_dir().join("autostart/kairos.desktop")
    }

    pub fn enabled() -> bool {
        std::fs::read_to_string(path()).map(|s| !s.contains("Hidden=true") && !s.contains("X-GNOME-Autostart-enabled=false")).unwrap_or(false)
    }

    pub fn set(on: bool) {
        if store::is_scratch() {
            return;
        }
        if on {
            let exe = exe_dir().join("kairos");
            let entry = format!(
                "[Desktop Entry]\nType=Application\nName=Kairos\nComment=Focus, tasks and app groups\nExec=\"{}\"\nIcon=kairos\nX-GNOME-Autostart-enabled=true\nNoDisplay=false\n",
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
