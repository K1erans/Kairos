use crate::models::AppData;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Loads and saves `AppData` as one JSON file. Saves are atomic.
pub struct DataStore {
    pub path: PathBuf,
}

/// `KAIROS_HOME` points the app and the native host at another folder (used for testing).
pub fn default_directory() -> PathBuf {
    if let Ok(home) = std::env::var("KAIROS_HOME") {
        if !home.is_empty() {
            return PathBuf::from(home);
        }
    }
    let base = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local/share"));
    base.join("kairos")
}

pub fn home_dir() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"))
}

pub fn config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".config"))
}

/// A scratch copy (`KAIROS_HOME` set) must leave the rest of the machine alone: the browsers'
/// native messaging manifests, autostart, the panel, the keyring, notifications, and other apps'
/// windows.
pub fn is_scratch() -> bool {
    std::env::var("KAIROS_HOME").map(|s| !s.is_empty()).unwrap_or(false)
}

pub fn encode(data: &AppData) -> Vec<u8> {
    // Through a Value, whose map keeps keys sorted, as the Mac app's `.sortedKeys` does.
    let value = serde_json::to_value(data).unwrap_or_default();
    serde_json::to_vec_pretty(&value).unwrap_or_default()
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

impl DataStore {
    pub fn new(path: PathBuf) -> Self {
        DataStore { path }
    }

    pub fn standard() -> Self {
        DataStore::new(default_directory().join("kairos.json"))
    }

    /// Returns starter data when no file exists. A file that fails to decode is kept beside the
    /// store as `kairos.corrupt-<time>.json` rather than overwritten.
    pub fn load(&self, starter: impl FnOnce() -> AppData) -> AppData {
        let Ok(raw) = fs::read(&self.path) else { return starter() };
        match serde_json::from_slice::<AppData>(&raw) {
            Ok(d) => d,
            Err(_) => {
                let stamp = chrono::Utc::now().timestamp();
                let backup = self.path.with_file_name(format!("kairos.corrupt-{stamp}.json"));
                let _ = fs::write(backup, &raw);
                starter()
            }
        }
    }

    pub fn save(&self, data: &AppData) -> std::io::Result<()> {
        write_atomic(&self.path, &encode(data))
    }
}
