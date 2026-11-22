use kairos_core::history::GroupSwitch;
use kairos_core::visibility::{GroupVisibilityTracker, Ownership, VisibilityApp};
use kairos_core::{AppGroup, SwitchBehavior};
use std::cell::RefCell;

fn work() -> AppGroup {
    let mut g = AppGroup::new("Work");
    g.dock_apps = vec!["editor.desktop".into()];
    g.blocked_apps = vec!["slack.desktop".into()];
    g
}
fn comms() -> AppGroup {
    let mut g = AppGroup::new("Comms");
    g.dock_apps = vec!["slack.desktop".into()];
    g
}
fn slack(is_hidden: bool) -> Vec<VisibilityApp<'static, String>> {
    vec![VisibilityApp { app: "slack".into(), app_id: Some("slack.desktop"), is_hidden }]
}
fn hide_slack() -> GroupSwitch {
    GroupSwitch::new(Some(&work()), SwitchBehavior::HideBlocked)
}
fn show_slack() -> GroupSwitch {
    GroupSwitch::new(Some(&comms()), SwitchBehavior::HideBlocked)
}
fn key() -> String {
    "slack".into()
}

#[test]
fn delayed_hide_confirmation_still_owns_and_restores_app() {
    let mut tracker = GroupVisibilityTracker::<String>::new();
    // The window manager does not confirm the hide at once.
    tracker.apply(&hide_slack(), &slack(false), &mut |_| false, &mut |_| true);
    assert_eq!(tracker.ownership.get(&key()), Some(&Ownership::HideRequested));

    let unhidden = RefCell::new(Vec::<String>::new());
    tracker.did_hide(&key(), &mut |a| {
        unhidden.borrow_mut().push(a.clone());
        true
    });
    assert_eq!(tracker.ownership.get(&key()), Some(&Ownership::Hidden));
    assert!(unhidden.borrow().is_empty());

    tracker.apply(&show_slack(), &slack(true), &mut |_| true, &mut |a| {
        unhidden.borrow_mut().push(a.clone());
        true
    });
    assert_eq!(*unhidden.borrow(), vec!["slack"]);
    assert_eq!(tracker.ownership.get(&key()), None);
}

#[test]
fn switching_back_before_delayed_hide_reverses_late_did_hide() {
    let mut tracker = GroupVisibilityTracker::<String>::new();
    tracker.apply(&hide_slack(), &slack(false), &mut |_| false, &mut |_| true);
    assert_eq!(tracker.ownership.get(&key()), Some(&Ownership::HideRequested));

    let unhidden = RefCell::new(Vec::<String>::new());
    tracker.apply(&show_slack(), &slack(false), &mut |_| true, &mut |a| {
        unhidden.borrow_mut().push(a.clone());
        true
    });
    assert_eq!(tracker.ownership.get(&key()), Some(&Ownership::RestoreRequested));
    assert!(unhidden.borrow().is_empty());

    tracker.did_hide(&key(), &mut |a| {
        unhidden.borrow_mut().push(a.clone());
        true
    });
    assert_eq!(*unhidden.borrow(), vec!["slack"]);
    assert_eq!(tracker.ownership.get(&key()), None);
}

#[test]
fn restore_all_before_delayed_hide_reverses_late_did_hide() {
    let mut tracker = GroupVisibilityTracker::<String>::new();
    tracker.apply(&hide_slack(), &slack(false), &mut |_| false, &mut |_| true);
    let unhidden = RefCell::new(Vec::<String>::new());
    tracker.restore_all(&|_| false, &mut |a| {
        unhidden.borrow_mut().push(a.clone());
        true
    });
    assert_eq!(tracker.ownership.get(&key()), Some(&Ownership::RestoreRequested));
    tracker.did_hide(&key(), &mut |a| {
        unhidden.borrow_mut().push(a.clone());
        true
    });
    assert_eq!(*unhidden.borrow(), vec!["slack"]);
    assert_eq!(tracker.ownership.get(&key()), None);
}

#[test]
fn manually_hidden_app_stays_unowned_and_untouched() {
    let mut tracker = GroupVisibilityTracker::<String>::new();
    let unhidden = RefCell::new(Vec::<String>::new());
    tracker.did_hide(&key(), &mut |a| {
        unhidden.borrow_mut().push(a.clone());
        true
    });
    assert_eq!(tracker.ownership.get(&key()), None);

    tracker.apply(&hide_slack(), &slack(true), &mut |_| panic!("already hidden"), &mut |a| {
        unhidden.borrow_mut().push(a.clone());
        true
    });
    assert_eq!(tracker.ownership.get(&key()), None);

    tracker.apply(&show_slack(), &slack(true), &mut |_| true, &mut |a| {
        unhidden.borrow_mut().push(a.clone());
        true
    });
    assert!(unhidden.borrow().is_empty(), "a manually hidden app must not be unhidden on group switch");
}

#[test]
fn failed_unhide_remains_owned_and_retries() {
    let mut tracker = GroupVisibilityTracker::<String>::new();
    tracker.apply(&hide_slack(), &slack(false), &mut |_| true, &mut |_| true);
    assert_eq!(tracker.ownership.get(&key()), Some(&Ownership::Hidden));

    tracker.apply(&show_slack(), &slack(true), &mut |_| true, &mut |_| false);
    assert_eq!(tracker.ownership.get(&key()), Some(&Ownership::RestoreRequested));

    let mut retried = false;
    tracker.restore_all(&|_| true, &mut |_| {
        retried = true;
        true
    });
    assert!(retried);
    assert_eq!(tracker.ownership.get(&key()), None);
}

#[test]
fn is_hidden_by_switch_tracks_only_kairos_hidden_apps() {
    let mut tracker = GroupVisibilityTracker::<String>::new();
    let hide_others = GroupSwitch::new(Some(&work()), SwitchBehavior::Hide);
    let notes_key = "notes".to_string();
    let notes = vec![VisibilityApp { app: notes_key.clone(), app_id: Some("notes.desktop"), is_hidden: false }];

    assert!(!tracker.is_hidden_by_switch(&notes_key));
    tracker.apply(&hide_others, &notes, &mut |_| true, &mut |_| true);
    assert!(tracker.is_hidden_by_switch(&notes_key));

    tracker.did_unhide(&notes_key, false);
    assert!(!tracker.is_hidden_by_switch(&notes_key));

    tracker.did_hide(&notes_key, &mut |_| true);
    assert!(!tracker.is_hidden_by_switch(&notes_key));
}
