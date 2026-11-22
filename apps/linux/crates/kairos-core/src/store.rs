use crate::models::AppData;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Loads and saves `AppData` as one JSON file. Saves are atomic.
///
/// When `load` could not read the file (anything but "no file yet"), or could not keep a copy
/// of a file it could not decode, the store becomes read-only: `save` refuses, so the starter
/// data `load` handed back never overwrites the owner's real file. `problem()` says why.
pub struct DataStore {
    pub path: PathBuf,
    problem: Mutex<Option<String>>,
}

/// Why `DataStore::try_load` could not hand back the saved data.
#[derive(Debug)]
pub enum LoadError {
    /// The file exists but could not be read (permissions, I/O error, a folder in its place).
    Unreadable(std::io::Error),
    /// The file could not be decoded, and the copy kept beside it could not be written.
    BackupFailed(std::io::Error),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Unreadable(e) => write!(f, "Kairos couldn\u{2019}t read its data file ({e}), so it won\u{2019}t save over it."),
            LoadError::BackupFailed(e) => write!(
                f,
                "Kairos couldn\u{2019}t read its data file and couldn\u{2019}t keep a copy of it ({e}), so it won\u{2019}t save over it."
            ),
        }
    }
}

impl std::error::Error for LoadError {}

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
        DataStore { path, problem: Mutex::new(None) }
    }

    pub fn standard() -> Self {
        DataStore::new(default_directory().join("kairos.json"))
    }

    /// Returns starter data when no file exists. A file that fails to decode is kept beside the
    /// store as `kairos.corrupt-<time>.json`, and starter data is returned. Any other failure
    /// (the file can't be read, or the copy can't be written) also returns starter data, but
    /// leaves the store read-only so later saves don't overwrite the file; see `problem()`.
    pub fn load(&self, starter: impl FnOnce() -> AppData) -> AppData {
        let mut starter = Some(starter);
        match self.try_load(|| (starter.take().unwrap())()) {
            Ok(d) => d,
            Err(_) => starter.take().map(|f| f()).unwrap_or_default(),
        }
    }

    /// As `load`, but says when the saved data could not be loaded. On `Err` the store is
    /// read-only.
    pub fn try_load(&self, starter: impl FnOnce() -> AppData) -> Result<AppData, LoadError> {
        let raw = match fs::read(&self.path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(starter()),
            Err(e) => {
                let err = LoadError::Unreadable(e);
                self.set_problem(err.to_string());
                return Err(err);
            }
        };
        match serde_json::from_slice::<AppData>(&raw) {
            Ok(d) => Ok(d),
            Err(_) => {
                let stamp = chrono::Utc::now().timestamp();
                let backup = self.path.with_file_name(format!("kairos.corrupt-{stamp}.json"));
                match write_atomic(&backup, &raw) {
                    Ok(()) => Ok(starter()),
                    Err(e) => {
                        let err = LoadError::BackupFailed(e);
                        self.set_problem(err.to_string());
                        Err(err)
                    }
                }
            }
        }
    }

    fn set_problem(&self, message: String) {
        *self.problem.lock().unwrap_or_else(|p| p.into_inner()) = Some(message);
    }

    /// Why the store is read-only, or None when saves go through.
    pub fn problem(&self) -> Option<String> {
        self.problem.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn is_read_only(&self) -> bool {
        self.problem().is_some()
    }

    /// Lets saves overwrite the file again, for when the owner chooses to replace it (for
    /// example by importing a file, or starting fresh, after being shown `problem()`).
    pub fn allow_overwrite(&self) {
        *self.problem.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    /// Refuses with `PermissionDenied` while the store is read-only (see `load`).
    pub fn save(&self, data: &AppData) -> std::io::Result<()> {
        if let Some(problem) = self.problem() {
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, problem));
        }
        write_atomic(&self.path, &encode(data))
    }
}
