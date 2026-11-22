use crate::models::AppData;
use crate::store::DataStore;
use crate::time::{local, Date};
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum ImportProblem {
    /// Not JSON, or JSON that is not a Kairos export.
    NotAnExport,
    /// Import waits for the timer to stop, so it cannot end or change a focus phase.
    TimerRunning,
}

impl std::fmt::Display for ImportProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ImportProblem::NotAnExport => "The file is not a Kairos export.",
            ImportProblem::TimerRunning => "Stop the timer first.",
        })
    }
}

impl AppData {
    /// Reads a file made by "Export everything as JSON" (on Linux or the Mac). Decoding is
    /// lenient, so any JSON object would read as an empty profile; a file without groups is
    /// refused instead.
    pub fn from_export(raw: &[u8]) -> Result<AppData, ImportProblem> {
        match serde_json::from_slice::<AppData>(raw) {
            Ok(file) if !file.groups.is_empty() => Ok(file),
            _ => Err(ImportProblem::NotAnExport),
        }
    }

    /// What Import puts in place of this data: everything from `file`, except what is live on
    /// this machine: the mode and the timer's saved state, the open session, the extensions seen
    /// here and onboarding.
    pub fn importing(&self, file: AppData, now: Date) -> AppData {
        let mut next = file;
        next.focus.mode = self.focus.mode;
        next.focus.ends_at = self.focus.ends_at;
        next.focus.strict_until = self.focus.strict_until;
        next.open_session = self.open_session.clone();
        next.seen_clients = self.seen_clients.clone();
        next.onboarded = self.onboarded;
        if next.group(next.focus.active_group_id).is_none() {
            next.focus.active_group_id = next.groups.first().map(|g| g.id);
        }
        if let Some(id) = next.focus.current_task_id {
            if !next.tasks.iter().any(|t| t.id == id) {
                next.focus.current_task_id = None;
            }
        }
        next.trim_history(now);
        next
    }
}

impl DataStore {
    /// Copies the saved file to `kairos-backup-<date>-<time>.json` beside it, before an import
    /// replaces it. None when there is no saved file yet.
    pub fn backup(&self, now: Date) -> std::io::Result<Option<PathBuf>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let stamp = local(now).format("%Y%m%d-%H%M%S").to_string();
        let dir = self.path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let mut target = dir.join(format!("kairos-backup-{stamp}.json"));
        let mut n = 2;
        while target.exists() {
            target = dir.join(format!("kairos-backup-{stamp}-{n}.json"));
            n += 1;
        }
        std::fs::copy(&self.path, &target)?;
        Ok(Some(target))
    }
}
