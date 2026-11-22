//! Per-app ownership state machine for group-switch window visibility. Tracks only apps Kairos
//! asked the window manager to hide, so apps the owner minimised stay untouched, and reconciles
//! late hide/unhide notifications without wall-clock timeouts.

use crate::history::{GroupSwitch, SwitchAction};
use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ownership {
    /// Kairos asked to hide this app and is waiting for the confirmation.
    HideRequested,
    /// Kairos owns this app's hidden state.
    Hidden,
    /// Kairos asked to restore this app while a hide was pending or after unhide failed.
    RestoreRequested,
}

pub struct VisibilityApp<'a, A> {
    pub app: A,
    pub app_id: Option<&'a str>,
    pub is_hidden: bool,
}

#[derive(Clone, Debug, Default)]
pub struct GroupVisibilityTracker<A: Hash + Eq + Clone> {
    pub ownership: HashMap<A, Ownership>,
}

impl<A: Hash + Eq + Clone> GroupVisibilityTracker<A> {
    pub fn new() -> Self {
        GroupVisibilityTracker { ownership: HashMap::new() }
    }

    pub fn apply(&mut self, policy: &GroupSwitch, apps: &[VisibilityApp<A>], hide: &mut dyn FnMut(&A) -> bool, unhide: &mut dyn FnMut(&A) -> bool) {
        let live: std::collections::HashSet<&A> = apps.iter().map(|a| &a.app).collect();
        self.ownership.retain(|k, _| live.contains(k));
        let mut to_restore: Vec<(A, bool)> = vec![];
        for entry in apps {
            let owned = self.ownership.contains_key(&entry.app);
            match policy.action(entry.app_id, entry.is_hidden, owned) {
                Some(SwitchAction::Hide) => {
                    let state = if hide(&entry.app) { Ownership::Hidden } else { Ownership::HideRequested };
                    self.ownership.insert(entry.app.clone(), state);
                }
                Some(SwitchAction::Unhide) => to_restore.push((entry.app.clone(), entry.is_hidden)),
                None => {
                    if entry.is_hidden {
                        if let Some(state) = self.ownership.get(&entry.app) {
                            if *state == Ownership::HideRequested || *state == Ownership::RestoreRequested {
                                self.ownership.insert(entry.app.clone(), Ownership::Hidden);
                            }
                        }
                    }
                }
            }
        }
        for (app, hidden) in to_restore {
            self.restore_app(&app, hidden, unhide);
        }
    }

    pub fn did_hide(&mut self, app: &A, unhide: &mut dyn FnMut(&A) -> bool) {
        match self.ownership.get(app) {
            Some(Ownership::HideRequested) => {
                self.ownership.insert(app.clone(), Ownership::Hidden);
            }
            Some(Ownership::RestoreRequested) => {
                if unhide(app) {
                    self.ownership.remove(app);
                } else {
                    self.ownership.insert(app.clone(), Ownership::Hidden);
                }
            }
            _ => {}
        }
    }

    pub fn did_unhide(&mut self, app: &A, is_hidden: bool) {
        if is_hidden {
            return;
        }
        if matches!(self.ownership.get(app), Some(Ownership::Hidden) | Some(Ownership::RestoreRequested)) {
            self.ownership.remove(app);
        }
    }

    pub fn did_terminate(&mut self, app: &A) {
        self.ownership.remove(app);
    }

    /// Restores all apps Kairos owns (on shutdown or when hiding is turned off).
    pub fn restore_all(&mut self, is_hidden: &dyn Fn(&A) -> bool, unhide: &mut dyn FnMut(&A) -> bool) {
        let apps: Vec<A> = self.ownership.keys().cloned().collect();
        for app in apps {
            let hidden = is_hidden(&app);
            self.restore_app(&app, hidden, unhide);
        }
    }

    fn restore_app(&mut self, app: &A, is_hidden: bool, unhide: &mut dyn FnMut(&A) -> bool) {
        match self.ownership.get(app).copied() {
            Some(Ownership::HideRequested) | Some(Ownership::RestoreRequested) => {
                if is_hidden {
                    if unhide(app) {
                        self.ownership.remove(app);
                    } else {
                        self.ownership.insert(app.clone(), Ownership::RestoreRequested);
                    }
                } else {
                    // The hide has not landed yet; a late did_hide is reversed.
                    self.ownership.insert(app.clone(), Ownership::RestoreRequested);
                }
            }
            Some(Ownership::Hidden) => {
                if is_hidden {
                    if unhide(app) {
                        self.ownership.remove(app);
                    } else {
                        self.ownership.insert(app.clone(), Ownership::RestoreRequested);
                    }
                } else {
                    self.ownership.remove(app);
                }
            }
            None => {}
        }
    }
}
