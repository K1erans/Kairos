//! Replacing Cinnamon's panel with Kairos Bar, the Linux counterpart of hiding the macOS Dock.
//! Kairos sets every panel to auto-hide (org.cinnamon panels-autohide) with a long show delay,
//! saving the owner's values first. A small restore script puts them back, and a systemd user
//! timer runs it whenever Kairos is not running, so a crash never leaves the panel hidden.
//!
//! `verification/system-dock/CinnamonPanel.tla` checks this. Four things keep the owner's panel
//! safe there, each named after the model's switch:
//! - RecoversFromScript: with nothing saved, `replace()` and `restore()` first run a leftover
//!   restore script, which holds the owner's values.
//! - RefusesOwnValues: `replace()` never records Kairos's own hidden setting as the owner's.
//! - ScriptRemovesItself: the script deletes itself and stops the timer once it has run, so it
//!   cannot keep undoing panel changes the owner makes later.
//! - ScriptLocked: the script and `replace()`/`restore()` share a lock (`panel.lock`), so a
//!   Kairos that starts while the script runs waits for it instead of racing it.

use crate::store;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

pub const SCHEMA: &str = "org.cinnamon";
pub const AUTOHIDE: &str = "panels-autohide";
pub const SHOW_DELAY: &str = "panels-show-delay";
pub const ENABLED: &str = "panels-enabled";
/// Long enough that brushing the edge does not bring the panel back over Kairos Bar.
pub const HIDDEN_SHOW_DELAY: &str = "['1:2000']";

/// Parses a gsettings string array such as `['1:true', '2:false']`. `@as []` is empty.
pub fn parse_array(text: &str) -> Vec<String> {
    let t = text.trim().trim_start_matches("@as").trim();
    let Some(inner) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else { return vec![] };
    let mut out = vec![];
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in inner.chars() {
        if escaped {
            cur.push(c);
            escaped = false;
            continue;
        }
        match quote {
            Some(q) => {
                if c == '\\' {
                    escaped = true;
                } else if c == q {
                    out.push(std::mem::take(&mut cur));
                    quote = None;
                } else {
                    cur.push(c);
                }
            }
            None => {
                if c == '\'' || c == '"' {
                    quote = Some(c);
                }
            }
        }
    }
    out
}

pub fn format_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|i| format!("'{}'", i.replace('\\', "\\\\").replace('\'', "\\'"))).collect();
    format!("[{}]", quoted.join(", "))
}

/// Panel ids from panels-enabled entries like `1:0:bottom` (panel:monitor:position).
pub fn panel_ids(enabled: &[String]) -> Vec<String> {
    enabled.iter().filter_map(|e| e.split(':').next()).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// panels-autohide with every panel set to auto-hide.
pub fn autohide_all(ids: &[String]) -> Vec<String> {
    ids.iter().map(|id| format!("{id}:true")).collect()
}

pub fn show_delay_all(ids: &[String], ms: u32) -> Vec<String> {
    ids.iter().map(|id| format!("{id}:{ms}")).collect()
}

/// The owner's settings before Kairos changed them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub autohide: String,
    pub show_delay: String,
}

pub fn saved_path() -> PathBuf {
    store::default_directory().join("panel-saved.json")
}

pub fn restore_script_path() -> PathBuf {
    store::default_directory().join("restore-panel.sh")
}

pub fn lock_path() -> PathBuf {
    store::default_directory().join("panel.lock")
}

/// Quotes `value` for sh as one word that sh expands nothing in: `'...'`, with each `'` written
/// as `'\''`.
pub fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// The restore script: puts the saved values back unless Kairos is running, then removes the
/// saved values, stops the timer and deletes itself. It holds `panel.lock` from before its
/// check until it ends. `--force` is Kairos running it while already holding the lock. The
/// check only counts this user's Kairos, and every value and path is single-quoted, so nothing
/// in a saved gsettings value (`$`, a backtick, `\`) is run or expanded by sh.
pub fn restore_script(saved: &Saved) -> String {
    format!(
        "#!/bin/sh\n# Written by Kairos. Restores Cinnamon's panel if Kairos is not running.\n\
         if [ \"$1\" != \"--force\" ]; then\n\
         \x20 exec 9>{lock}\n\
         \x20 flock 9\n\
         \x20 if pgrep -u \"$(id -u)\" -x kairos >/dev/null 2>&1; then exit 0; fi\n\
         fi\n\
         gsettings set {SCHEMA} {AUTOHIDE} {autohide}\n\
         gsettings set {SCHEMA} {SHOW_DELAY} {delay}\n\
         rm -f {saved}\n\
         systemctl --user disable --now {UNIT}.timer >/dev/null 2>&1\n\
         rm -f \"$0\"\n",
        lock = sh_quote(&lock_path().display().to_string()),
        autohide = sh_quote(&saved.autohide),
        delay = sh_quote(&saved.show_delay),
        saved = sh_quote(&saved_path().display().to_string())
    )
}

/// Whether these are the values `replace()` sets: every panel auto-hidden with Kairos's delay.
pub fn is_own_setting(autohide: &str, show_delay: &str) -> bool {
    let hide = parse_array(autohide);
    let delay = parse_array(show_delay);
    !hide.is_empty() && hide.iter().all(|e| e.ends_with(":true")) && !delay.is_empty() && delay.iter().all(|e| e.ends_with(":2000"))
}

pub const UNIT: &str = "kairos-panel-watchdog";

pub fn unit_dir() -> PathBuf {
    store::config_dir().join("systemd/user")
}

pub fn service_unit() -> String {
    format!(
        "[Unit]\nDescription=Restore Cinnamon's panel if Kairos is not running\n\n[Service]\nType=oneshot\nExecStart=/bin/sh {}\n",
        restore_script_path().display()
    )
}

pub fn timer_unit() -> String {
    "[Unit]\nDescription=Check Cinnamon's panel every 30 seconds while Kairos replaces it\n\n[Timer]\nOnBootSec=20\nOnUnitActiveSec=30\nAccuracySec=5\n\n[Install]\nWantedBy=timers.target\n".into()
}

fn gsettings_get(key: &str) -> Option<String> {
    let out = Command::new("gsettings").args(["get", SCHEMA, key]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn gsettings_reset(key: &str) {
    let _ = Command::new("gsettings").args(["reset", SCHEMA, key]).status();
}

fn gsettings_set(key: &str, value: &str) -> bool {
    Command::new("gsettings").args(["set", SCHEMA, key, value]).status().map(|s| s.success()).unwrap_or(false)
}

/// Whether Cinnamon's settings are there to change.
pub fn available() -> bool {
    gsettings_get(ENABLED).is_some()
}

#[derive(Debug, PartialEq)]
pub enum PanelError {
    NotCinnamon,
    Scratch,
    Failed(String),
}

impl std::fmt::Display for PanelError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            PanelError::NotCinnamon => write!(f, "Cinnamon's panel settings were not found, so Kairos can't hide the panel."),
            PanelError::Scratch => write!(f, "Kairos is using a test profile (KAIROS_HOME), so it leaves the panel alone."),
            PanelError::Failed(m) => write!(f, "Couldn't change the panel: {m}"),
        }
    }
}

/// Holds `panel.lock`, shared with the restore script, until dropped.
struct PanelLock(Option<std::fs::File>);

fn lock() -> PanelLock {
    let _ = std::fs::create_dir_all(store::default_directory());
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(lock_path()).ok();
    // Without a lock file, carry on: hiding or restoring the panel matters more than the race.
    PanelLock(file.filter(|f| f.lock().is_ok()))
}

/// With nothing saved, a restore script left on disk holds the owner's values (Kairos crashed
/// with them saved, then lost the file). Run it to put them back before going on.
fn recover() {
    let script = restore_script_path();
    if load_saved().is_none() && script.exists() {
        let _ = Command::new("/bin/sh").arg(&script).arg("--force").status();
    }
}

/// Hides Cinnamon's panels behind Kairos Bar, saving the owner's settings the first time.
pub fn replace() -> Result<(), PanelError> {
    if store::is_scratch() {
        return Err(PanelError::Scratch);
    }
    let enabled = gsettings_get(ENABLED).ok_or(PanelError::NotCinnamon)?;
    let ids = panel_ids(&parse_array(&enabled));
    let _lock = lock();
    recover();
    let saved = match load_saved() {
        Some(s) => s,
        None => {
            let mut autohide = gsettings_get(AUTOHIDE).unwrap_or_else(|| "[]".into());
            let mut show_delay = gsettings_get(SHOW_DELAY).unwrap_or_else(|| "[]".into());
            if is_own_setting(&autohide, &show_delay) {
                // Kairos's own setting is on the panel and the owner's is lost: record Cinnamon's
                // default rather than make hiding everything the owner's choice.
                gsettings_reset(AUTOHIDE);
                gsettings_reset(SHOW_DELAY);
                autohide = gsettings_get(AUTOHIDE).unwrap_or_else(|| "[]".into());
                show_delay = gsettings_get(SHOW_DELAY).unwrap_or_else(|| "[]".into());
            }
            let s = Saved { autohide, show_delay };
            let json = serde_json::to_vec_pretty(&s).map_err(|e| PanelError::Failed(e.to_string()))?;
            store::write_atomic(&saved_path(), &json).map_err(|e| PanelError::Failed(e.to_string()))?;
            s
        }
    };
    install_watchdog(&saved).map_err(PanelError::Failed)?;
    if !gsettings_set(AUTOHIDE, &format_array(&autohide_all(&ids))) {
        return Err(PanelError::Failed("gsettings refused panels-autohide".into()));
    }
    gsettings_set(SHOW_DELAY, &format_array(&show_delay_all(&ids, 2000)));
    Ok(())
}

/// Puts the owner's panel settings back and removes the watchdog.
pub fn restore() {
    if store::is_scratch() {
        return;
    }
    let _lock = lock();
    recover();
    if let Some(saved) = load_saved() {
        gsettings_set(AUTOHIDE, &saved.autohide);
        gsettings_set(SHOW_DELAY, &saved.show_delay);
        let _ = std::fs::remove_file(saved_path());
    }
    remove_watchdog();
}

pub fn is_replaced() -> bool {
    saved_path().exists()
}

fn load_saved() -> Option<Saved> {
    let bytes = std::fs::read(saved_path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn install_watchdog(saved: &Saved) -> Result<(), String> {
    let script = restore_script_path();
    store::write_atomic(&script, restore_script(saved).as_bytes()).map_err(|e| e.to_string())?;
    let dir = unit_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    store::write_atomic(&dir.join(format!("{UNIT}.service")), service_unit().as_bytes()).map_err(|e| e.to_string())?;
    store::write_atomic(&dir.join(format!("{UNIT}.timer")), timer_unit().as_bytes()).map_err(|e| e.to_string())?;
    let _ = Command::new("systemctl").args(["--user", "daemon-reload"]).status();
    let _ = Command::new("systemctl").args(["--user", "enable", "--now", &format!("{UNIT}.timer")]).status();
    Ok(())
}

fn remove_watchdog() {
    let _ = Command::new("systemctl").args(["--user", "disable", "--now", &format!("{UNIT}.timer")]).status();
    let dir = unit_dir();
    let _ = std::fs::remove_file(dir.join(format!("{UNIT}.service")));
    let _ = std::fs::remove_file(dir.join(format!("{UNIT}.timer")));
    let _ = std::fs::remove_file(restore_script_path());
    let _ = Command::new("systemctl").args(["--user", "daemon-reload"]).status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_arrays() {
        assert_eq!(parse_array("['1:true', '2:false']"), vec!["1:true", "2:false"]);
        assert!(parse_array("@as []").is_empty());
        assert_eq!(format_array(&["1:true".into()]), "['1:true']");
        assert_eq!(panel_ids(&parse_array("['1:0:bottom', '2:1:left']")), vec!["1", "2"]);
        assert_eq!(autohide_all(&["1".into()]), vec!["1:true"]);
    }

    #[test]
    fn knows_its_own_setting() {
        assert!(is_own_setting("['1:true', '2:true']", "['1:2000', '2:2000']"));
        assert!(!is_own_setting("['1:false']", "['1:2000']"));
        assert!(!is_own_setting("['1:true']", "['1:0']"));
        assert!(!is_own_setting("@as []", "@as []"));
    }

    #[test]
    fn script_locks_checks_and_removes_itself() {
        let s = restore_script(&Saved { autohide: "['1:false']".into(), show_delay: "['1:0']".into() });
        let lock = s.find("flock 9").unwrap();
        let check = s.find("pgrep -u \"$(id -u)\" -x kairos").unwrap();
        let set = s.find("gsettings set").unwrap();
        assert!(lock < check && check < set, "{s}");
        assert!(s.contains("disable --now kairos-panel-watchdog.timer"));
        assert!(s.trim_end().ends_with("rm -f \"$0\""));
        if let Ok(out) = std::env::var("KAIROS_DUMP_RESTORE_SCRIPT") {
            std::fs::write(out, &s).unwrap();
        }
        let check = Command::new("sh").arg("-n").stdin(std::process::Stdio::piped()).spawn().and_then(|mut c| {
            use std::io::Write;
            c.stdin.take().unwrap().write_all(s.as_bytes())?;
            c.wait()
        });
        assert!(check.map(|st| st.success()).unwrap_or(true), "not valid sh:\n{s}");
    }
}
