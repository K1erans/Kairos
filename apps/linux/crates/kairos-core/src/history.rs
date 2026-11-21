use crate::id::Id;
use crate::models::{AppData, AppGroup, BlockEvent, FocusState, Session, SessionKind, SwitchBehavior, TaskStatus};
use crate::tempo::{StrictResume, TempoEvent, TempoPhase};
use crate::time::{add, since, Date};
use std::collections::HashSet;

/// Shorter phases are false starts and are not logged.
pub const MINIMUM_LENGTH: f64 = 30.0;

impl Session {
    /// The session a finished timer phase leaves in the log, or None for anything else.
    pub fn from_event(event: &TempoEvent, task_id: Option<Id>, group_id: Option<Id>) -> Option<Session> {
        let TempoEvent::Finished { phase, start, end, duration, interruptions, completed } = event else { return None };
        if *phase == TempoPhase::Idle || *duration < MINIMUM_LENGTH {
            return None;
        }
        let focus = *phase == TempoPhase::Focus;
        let mut s = Session::new(
            if focus { task_id } else { None },
            group_id,
            *start,
            *end,
            if focus { SessionKind::Focus } else { SessionKind::Rest },
        );
        s.active_seconds = Some(*duration);
        s.interruptions = *interruptions;
        s.completed = *completed;
        Some(s)
    }
}

pub const SESSION_DAYS: i64 = 400;
pub const MAX_BLOCK_EVENTS: usize = 5000;
pub const MAX_SESSIONS: usize = 20_000;

impl AppData {
    /// Logs a finished timer phase against the current task and group.
    pub fn log(&mut self, event: &TempoEvent, now: Date) {
        if let Some(s) = Session::from_event(event, self.focus.current_task_id, self.focus.active_group_id) {
            self.sessions.push(s);
            self.trim_history(now);
        }
    }

    pub fn log_block(&mut self, event: BlockEvent, now: Date) {
        self.block_events.push(event);
        self.trim_history(now);
    }

    pub fn trim_history(&mut self, now: Date) {
        let cutoff = add(now, -(SESSION_DAYS as f64) * 86400.0);
        self.sessions.retain(|s| s.end >= cutoff);
        if self.sessions.len() > MAX_SESSIONS {
            let n = self.sessions.len() - MAX_SESSIONS;
            self.sessions.drain(..n);
        }
        self.block_events.retain(|e| e.time >= cutoff);
        if self.block_events.len() > MAX_BLOCK_EVENTS {
            let n = self.block_events.len() - MAX_BLOCK_EVENTS;
            self.block_events.drain(..n);
        }
    }

    /// Run once at launch. A strict focus phase that has not run out picks up where it was, so
    /// quitting or a crash cannot end it. Any other timer does not survive a quit.
    pub fn recover_after_launch(&mut self, now: Date) -> Option<StrictResume> {
        self.trim_history(now);
        if let Some(until) = self.focus.strict_until {
            if until > now {
                let focus_open = self.open_session.as_ref().filter(|s| s.kind == SessionKind::Focus);
                let start = focus_open.map(|s| s.start).unwrap_or(now);
                let paused = focus_open.map(|s| (since(s.end, s.start) - s.duration()).max(0.0)).unwrap_or(0.0);
                self.open_session = None;
                self.focus.mode = crate::models::Mode::Strict;
                self.focus.ends_at = Some(until);
                return Some(StrictResume::new(start.min(now), until, paused));
            }
        }
        if let Some(open) = self.open_session.clone() {
            if open.duration() >= MINIMUM_LENGTH {
                let mut cut = open;
                cut.completed = false;
                self.sessions.push(cut);
                self.trim_history(now);
            }
        }
        self.open_session = None;
        self.focus.mode = crate::models::Mode::Off;
        self.focus.ends_at = None;
        self.focus.strict_until = None;
        None
    }

    /// Marks the newest unmarked block of `target` as overridden. An app id marks only that app;
    /// a site also marks blocks of its subdomains (see `rules::target_matches`).
    pub fn mark_overridden(&mut self, target: &str) {
        if let Some(e) = self
            .block_events
            .iter_mut()
            .rev()
            .find(|e| !e.overridden && crate::rules::target_matches(&e.target, target))
        {
            e.overridden = true;
        }
    }

    /// Kairos Read time goes to the current task, or to an unassigned total.
    pub fn log_listening(&mut self, seconds: i64) {
        if seconds <= 0 {
            return;
        }
        let current = self.focus.current_task_id;
        if let Some(t) = self.tasks.iter_mut().find(|t| Some(t.id) == current) {
            t.listened_sec += seconds;
        } else {
            self.unassigned_listened_sec += seconds;
        }
    }

    /// The focus state after switching to group `id`.
    pub fn focus_after_switch(&self, id: Id) -> FocusState {
        let mut focus = self.focus.clone();
        if let Some(old) = focus.active_group_id {
            match focus.current_task_id {
                Some(t) => {
                    focus.task_by_group.insert(old.string(), t);
                }
                None => {
                    focus.task_by_group.remove(&old.string());
                }
            }
        }
        focus.active_group_id = Some(id);
        if self.current_task().and_then(|t| t.group_id) != Some(id) {
            let remembered = focus.task_by_group.get(&id.string()).and_then(|tid| self.tasks.iter().find(|t| t.id == *tid));
            focus.current_task_id = remembered.filter(|t| t.status == TaskStatus::Todo && t.group_id == Some(id)).map(|t| t.id);
        }
        focus
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchAction {
    Hide,
    Unhide,
}

/// Window visibility when entering a group. Only apps Kairos hid for a group switch can be restored.
#[derive(Clone, Debug)]
pub struct GroupSwitch {
    behavior: SwitchBehavior,
    apps: HashSet<String>,
}

/// The file manager stays available, as Finder does on the Mac.
pub const FILE_MANAGERS: &[&str] = &["nemo.desktop", "org.gnome.Nautilus.desktop", "caja.desktop", "thunar.desktop"];

impl GroupSwitch {
    pub fn new(group: Option<&AppGroup>, behavior: SwitchBehavior) -> Self {
        let apps = match group {
            None => HashSet::new(),
            Some(g) => {
                let list = if behavior == SwitchBehavior::HideBlocked { &g.blocked_apps } else { &g.dock_apps };
                list.iter().cloned().collect()
            }
        };
        GroupSwitch { behavior: if group.is_none() { SwitchBehavior::Leave } else { behavior }, apps }
    }

    pub fn action(&self, app: Option<&str>, is_hidden: bool, hidden_by_switch: bool) -> Option<SwitchAction> {
        let listed = app.map(|a| self.apps.contains(a)).unwrap_or(false);
        let should_hide = match self.behavior {
            SwitchBehavior::Leave => false,
            SwitchBehavior::HideBlocked => listed,
            SwitchBehavior::Hide => !listed,
        };
        if should_hide && !app.map(|a| FILE_MANAGERS.contains(&a)).unwrap_or(false) {
            return if is_hidden { None } else { Some(SwitchAction::Hide) };
        }
        if hidden_by_switch {
            Some(SwitchAction::Unhide)
        } else {
            None
        }
    }
}
