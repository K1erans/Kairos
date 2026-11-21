//! Replacing Cinnamon's panel with Kairos Bar, the Linux counterpart of hiding the macOS Dock.
//! Kairos sets every panel to auto-hide (org.cinnamon panels-autohide) with a long show delay,
//! saving the owner's values first. A small restore script puts them back, and a systemd user
//! timer runs it whenever Kairos is not running, so a crash never leaves the panel hidden.

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

/// The restore script: puts the saved values back unless Kairos is running.
pub fn restore_script(saved: &Saved) -> String {
    format!(
        "#!/bin/sh\n# Written by Kairos. Restores Cinnamon's panel if Kairos is not running.\n\
         if pgrep -x kairos >/dev/null 2>&1 && [ \"$1\" != \"--force\" ]; then exit 0; fi\n\
         gsettings set {SCHEMA} {AUTOHIDE} \"{}\"\n\
         gsettings set {SCHEMA} {SHOW_DELAY} \"{}\"\n\
         rm -f \"{}\"\n",
        saved.autohide.replace('"', "\\\""),
        saved.show_delay.replace('"', "\\\""),
        saved_path().display()
    )
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

/// Hides Cinnamon's panels behind Kairos Bar, saving the owner's settings the first time.
pub fn replace() -> Result<(), PanelError> {
    if store::is_scratch() {
        return Err(PanelError::Scratch);
    }
    let enabled = gsettings_get(ENABLED).ok_or(PanelError::NotCinnamon)?;
    let ids = panel_ids(&parse_array(&enabled));
    let saved = match load_saved() {
        Some(s) => s,
        None => {
            let s = Saved {
                autohide: gsettings_get(AUTOHIDE).unwrap_or_else(|| "[]".into()),
                show_delay: gsettings_get(SHOW_DELAY).unwrap_or_else(|| "[]".into()),
            };
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
}
