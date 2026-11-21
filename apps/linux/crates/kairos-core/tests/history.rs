mod common;
use common::*;
use kairos_core::history::{GroupSwitch, SwitchAction, MAX_BLOCK_EVENTS, MAX_SESSIONS};
use kairos_core::store::DataStore;
use kairos_core::tempo::{TempoEngine, TempoEvent, TempoPhase};
use kairos_core::*;

const T0: f64 = 1_800_000_000.0;
fn at(s: f64) -> Date {
    t(T0 + s)
}
fn starter() -> AppData {
    AppData::starter(None, &|_| true)
}
fn classic() -> TempoPreset {
    TempoPreset::defaults().remove(0)
}
fn ev(target: &str, label: Option<&str>, layer: &str, overridden: bool, time: Date) -> BlockEvent {
    BlockEvent::new(None, target, label.map(String::from), layer, overridden, time)
}
fn store(tag: &str) -> DataStore {
    DataStore::new(temp_dir(tag).join("kairos.json"))
}

// MARK: HistoryTests

#[test]
fn override_marks_the_event_for_the_allowed_target() {
    let mut d = starter();
    d.log_block(ev("youtube.com", None, "rule", false, at(0.0)), at(0.0));
    d.log_block(ev("discord.desktop", Some("Discord"), "app", false, at(0.0)), at(0.0));
    d.log_block(ev("reddit.com", None, "rule", false, at(0.0)), at(0.0));
    d.mark_overridden("youtube.com");
    d.mark_overridden("discord.desktop");
    assert_eq!(d.block_events.iter().map(|e| e.overridden).collect::<Vec<_>>(), vec![true, true, false]);
    assert_eq!(d.block_events[1].display_name(), "Discord");
    assert_eq!(d.block_events[0].display_name(), "youtube.com");
}

#[test]
fn a_grant_covers_subdomain_events_and_skips_marked_ones() {
    let mut d = AppData::default();
    d.log_block(ev("m.youtube.com", None, "rule", false, at(0.0)), at(0.0));
    d.log_block(ev("youtube.com", None, "rule", true, at(0.0)), at(0.0));
    d.mark_overridden("youtube.com");
    assert_eq!(d.block_events.iter().map(|e| e.overridden).collect::<Vec<_>>(), vec![true, true]);
    d.mark_overridden("x.com");
    assert_eq!(d.block_events.iter().map(|e| e.overridden).collect::<Vec<_>>(), vec![true, true]);
}

#[test]
fn listening_goes_to_the_current_task_or_to_unassigned() {
    let mut d = starter();
    d.log_listening(30);
    assert_eq!(d.unassigned_listened_sec, 30);
    let task = TaskItem::new("Read", Some(d.groups[0].id));
    d.focus.current_task_id = Some(task.id);
    d.tasks.push(task);
    d.log_listening(20);
    d.log_listening(-5);
    assert_eq!(d.tasks[0].listened_sec, 20);
    assert_eq!(d.unassigned_listened_sec, 30);
}

#[test]
fn relaunch_logs_the_interrupted_phase_and_stops_blocking() {
    let store = store("relaunch");
    let mut e = TempoEngine::new(classic());
    e.start(TempoPhase::Focus, false, at(0.0));
    let mut d = starter();
    d.focus.mode = Mode::Focus;
    d.focus.ends_at = e.ends_at();
    d.open_session = Session::from_event(&e.current(at(600.0)).unwrap(), None, Some(d.groups[0].id));
    store.save(&d).unwrap();

    let mut back = store.load(starter);
    assert_eq!(back.recover_after_launch(at(3600.0)), None);
    assert_eq!(back.focus.mode, Mode::Off);
    assert_eq!(back.focus.ends_at, None);
    assert_eq!(back.open_session, None);
    assert_eq!(back.sessions.len(), 1);
    assert_eq!(back.sessions[0].end, at(600.0), "recorded up to the last save");
    assert!(!back.sessions[0].completed);
}

#[test]
fn relaunch_while_paused_comes_back_off() {
    let mut e = TempoEngine::new(classic());
    e.start(TempoPhase::Focus, false, at(0.0));
    e.pause(at(300.0));
    let mut d = starter();
    d.focus.mode = Mode::Focus;
    d.open_session = Session::from_event(&e.current(at(900.0)).unwrap(), None, None);
    d.recover_after_launch(at(1000.0));
    assert_eq!(d.focus.mode, Mode::Off);
    assert_eq!(d.sessions.first().map(|s| s.end), Some(at(300.0)), "a paused phase stopped counting when paused");
}

#[test]
fn false_starts_are_not_logged() {
    let mut d = AppData::default();
    d.log(
        &TempoEvent::Finished { phase: TempoPhase::Focus, start: at(0.0), end: at(10.0), duration: 10.0, interruptions: 0, completed: false },
        at(10.0),
    );
    d.log(&TempoEvent::Started(TempoPhase::Focus), at(10.0));
    assert!(d.sessions.is_empty());
    d.open_session = Some(Session::new(None, None, at(0.0), at(5.0), SessionKind::Focus));
    d.recover_after_launch(at(0.0));
    assert!(d.sessions.is_empty());
}

#[test]
fn history_is_capped() {
    let mut d = AppData::default();
    let day = 86400.0;
    d.sessions = vec![
        Session::new(None, None, at(-500.0 * day), at(-500.0 * day + 600.0), SessionKind::Focus),
        Session::new(None, None, at(-10.0 * day), at(-10.0 * day + 600.0), SessionKind::Focus),
    ];
    d.log(
        &TempoEvent::Finished { phase: TempoPhase::Focus, start: at(0.0), end: at(600.0), duration: 600.0, interruptions: 0, completed: true },
        at(600.0),
    );
    assert_eq!(d.sessions.len(), 2, "sessions older than 400 days go");
    for i in 0..(MAX_BLOCK_EVENTS + 10) {
        d.log_block(ev(&format!("s{i}.com"), None, "rule", false, at(0.0)), at(0.0));
    }
    assert_eq!(d.block_events.len(), MAX_BLOCK_EVENTS);
    assert_eq!(d.block_events.last().unwrap().target, format!("s{}.com", MAX_BLOCK_EVENTS + 9), "the newest are kept");
}

#[test]
fn group_switch_restores_only_apps_in_the_destination_group() {
    let mut work = AppGroup::new("Work");
    work.dock_apps = vec!["mail".into(), "browser".into(), "notes".into()];
    let policy = GroupSwitch::new(Some(&work), SwitchBehavior::Hide);
    assert_eq!(policy.action(Some("mail"), true, true), Some(SwitchAction::Unhide));
    assert_eq!(policy.action(Some("browser"), false, false), None);
    assert_eq!(policy.action(Some("notes"), true, false), None);
    assert_eq!(policy.action(Some("chat"), true, true), None);
    assert_eq!(policy.action(Some("music"), false, false), Some(SwitchAction::Hide));
    assert_eq!(policy.action(Some("nemo.desktop"), false, false), None, "the file manager stays");
}

#[test]
fn hiding_only_the_blocked_apps_leaves_every_other_app_alone() {
    let mut group = AppGroup::new("Study");
    group.dock_apps = vec!["chat".into()];
    group.blocked_apps = vec!["chat".into(), "mail".into()];
    let policy = GroupSwitch::new(Some(&group), SwitchBehavior::HideBlocked);
    assert_eq!(policy.action(Some("chat"), false, false), Some(SwitchAction::Hide));
    assert_eq!(policy.action(Some("mail"), true, true), None);
    assert_eq!(policy.action(Some("music"), false, false), None);
    assert_eq!(policy.action(Some("browser"), true, true), Some(SwitchAction::Unhide));
    assert_eq!(serde_json::from_str::<SwitchBehavior>("\"hideBlocked\"").unwrap(), SwitchBehavior::HideBlocked);
}

#[test]
fn paused_time_is_excluded_from_logged_focus_minutes() {
    let mut e = TempoEngine::new(classic());
    let mut d = AppData::default();
    e.start(TempoPhase::Focus, false, at(0.0));
    e.pause(at(600.0));
    e.resume(at(1200.0));
    e.pause(at(1500.0));
    e.resume(at(1800.0));
    for event in e.tick(at(2400.0), true) {
        d.log(&event, at(2400.0));
    }
    assert_eq!(d.sessions.len(), 1);
    assert!(approx(d.sessions[0].duration(), 1500.0, 0.001));
    assert!(approx(d.sessions[0].minutes(), 25.0, 0.001));
    assert_eq!(d.sessions[0].end, at(2400.0));
}

#[test]
fn false_start_with_long_pause_gap_is_not_logged() {
    let mut e = TempoEngine::new(classic());
    let mut d = AppData::default();
    e.start(TempoPhase::Focus, false, at(0.0));
    e.pause(at(10.0));
    e.resume(at(120.0));
    for event in e.stop(at(130.0)) {
        d.log(&event, at(130.0));
    }
    assert!(d.sessions.is_empty());
    let mut open = Session::new(None, None, at(0.0), at(130.0), SessionKind::Focus);
    open.active_seconds = Some(20.0);
    d.open_session = Some(open);
    d.recover_after_launch(at(200.0));
    assert!(d.sessions.is_empty());
}

#[test]
fn strict_relaunch_preserves_paused_duration() {
    let mut e = TempoEngine::new(classic());
    e.start(TempoPhase::Focus, true, at(0.0));
    e.pause(at(600.0));
    e.resume(at(1200.0));
    let mut d = AppData::default();
    d.focus.mode = Mode::Strict;
    d.focus.strict_until = e.ends_at();
    d.open_session = e.current(at(1500.0)).and_then(|ev| Session::from_event(&ev, None, None));
    let resumed = d.recover_after_launch(at(1600.0)).expect("strict phase must resume");
    assert!(approx(resumed.paused_duration, 600.0, 0.001));
    let mut relaunched = TempoEngine::new(classic());
    relaunched.resume_strict(&resumed);
    for event in relaunched.tick(at(2100.0), true) {
        d.log(&event, at(2100.0));
    }
    assert_eq!(d.sessions.len(), 1);
    assert!(approx(d.sessions[0].minutes(), 25.0, 0.001));
}

#[test]
fn legacy_session_without_active_seconds_falls_back_to_wall_clock() {
    let json = r#"{"start":"2026-01-01T10:00:00Z","end":"2026-01-01T10:25:00Z","kind":"focus","interruptions":0,"completed":true}"#;
    let s: Session = serde_json::from_str(json).unwrap();
    assert_eq!(s.active_seconds, None);
    assert!(approx(s.duration(), 1500.0, 0.001));
    assert!(approx(s.minutes(), 25.0, 0.001));
}

// MARK: HistoryCapTests

const NOW: f64 = 2_000_000_000.0;
fn days_ago(d: f64) -> Date {
    t(NOW - d * 86400.0)
}
fn session(end: Date) -> Session {
    Session::new(None, None, plus(end, -1500.0), end, SessionKind::Focus)
}

#[test]
fn launch_trims_history_even_when_nothing_new_is_logged() {
    let mut data = starter();
    data.sessions = vec![session(days_ago(500.0)), session(days_ago(1.0))];
    data.block_events = vec![ev("old.com", None, "rule", false, days_ago(500.0)), ev("new.com", None, "rule", false, days_ago(1.0))];
    data.recover_after_launch(t(NOW));
    assert_eq!(data.sessions.len(), 1);
    assert_eq!(data.block_events.iter().map(|e| e.target.as_str()).collect::<Vec<_>>(), vec!["new.com"], "block events age out too");
}

#[test]
fn an_old_session_out_of_order_is_still_trimmed() {
    let mut data = starter();
    data.sessions = vec![session(days_ago(1.0)), session(days_ago(500.0))];
    data.trim_history(t(NOW));
    assert_eq!(data.sessions.iter().map(|s| s.end).collect::<Vec<_>>(), vec![days_ago(1.0)]);
}

#[test]
fn sessions_are_capped_by_count_as_well_as_age() {
    let mut data = starter();
    data.sessions = (0..(MAX_SESSIONS + 10)).map(|i| session(t(NOW - i as f64))).rev().collect();
    data.trim_history(t(NOW));
    assert_eq!(data.sessions.len(), MAX_SESSIONS);
    assert_eq!(data.sessions.last().unwrap().end, t(NOW), "the newest are kept");
}

// MARK: StrictRelaunchTests

fn saved_mid_strict(until: Date, saved_at: Date) -> AppData {
    let mut e = TempoEngine::new(TempoPreset::defaults()[1].clone()); // Deep work: strict, 50 min
    e.start(TempoPhase::Focus, false, at(0.0));
    let mut d = starter();
    d.focus.mode = Mode::Strict;
    d.focus.ends_at = e.ends_at();
    d.focus.strict_until = Some(until);
    d.open_session = Session::from_event(&e.current(saved_at).unwrap(), None, Some(d.groups[0].id));
    let store = store("strict");
    store.save(&d).unwrap();
    store.load(starter)
}

#[test]
fn a_relaunch_before_the_end_resumes_the_strict_phase_with_the_time_left() {
    let mut d = saved_mid_strict(at(3000.0), at(600.0));
    let resume = d.recover_after_launch(at(900.0)).expect("resumes");
    assert_eq!(d.focus.mode, Mode::Strict);
    assert_eq!(d.focus.strict_until, Some(at(3000.0)));
    assert_eq!(resume.ends_at, at(3000.0));
    assert_eq!(resume.started_at, at(0.0), "the phase keeps its start, so it is logged once, whole");
    assert!(d.sessions.is_empty(), "the phase is still running, so nothing is logged yet");

    let mut tempo = TempoEngine::new(classic());
    tempo.resume_strict(&resume);
    assert_eq!(tempo.phase(), TempoPhase::Focus);
    assert_eq!(tempo.mode(), Mode::Strict);
    assert_eq!(tempo.remaining(at(900.0)), 2100.0);
    let events = tempo.tick(at(3000.0), true);
    assert_eq!(
        events.first(),
        Some(&TempoEvent::Finished { phase: TempoPhase::Focus, start: at(0.0), end: at(3000.0), duration: 3000.0, interruptions: 0, completed: true })
    );
    assert_eq!(tempo.phase(), TempoPhase::ShortBreak);
}

#[test]
fn a_relaunch_after_the_end_comes_back_off() {
    let mut d = saved_mid_strict(at(3000.0), at(600.0));
    assert_eq!(d.recover_after_launch(at(3001.0)), None);
    assert_eq!(d.focus.mode, Mode::Off);
    assert_eq!(d.focus.strict_until, None);
    assert_eq!(d.sessions.iter().map(|s| s.end).collect::<Vec<_>>(), vec![at(600.0)], "logged up to the last save, as for any other phase");
}

// MARK: GroupTaskTests

#[test]
fn switching_away_and_back_brings_the_task_back() {
    let mut d = AppData::default();
    let work = AppGroup::new("Work");
    let study = AppGroup::new("Study");
    let report = TaskItem::new("Write report", Some(work.id));
    let essay = TaskItem::new("Essay", Some(study.id));
    d.focus.active_group_id = Some(work.id);
    d.focus.current_task_id = Some(report.id);
    let (wid, sid, rid, eid) = (work.id, study.id, report.id, essay.id);
    d.groups = vec![work, study];
    d.tasks = vec![report, essay];

    d.focus = d.focus_after_switch(sid);
    assert_eq!(d.focus.current_task_id, None, "Study had no task yet");
    d.focus.current_task_id = Some(eid);
    d.focus = d.focus_after_switch(wid);
    assert_eq!(d.focus.current_task_id, Some(rid));
    d.focus = d.focus_after_switch(sid);
    assert_eq!(d.focus.current_task_id, Some(eid));
}

#[test]
fn a_done_or_deleted_task_does_not_come_back() {
    let mut d = AppData::default();
    let work = AppGroup::new("Work");
    let study = AppGroup::new("Study");
    let report = TaskItem::new("Write report", Some(work.id));
    d.focus.active_group_id = Some(work.id);
    d.focus.current_task_id = Some(report.id);
    let (wid, sid) = (work.id, study.id);
    d.groups = vec![work, study];
    d.tasks = vec![report];
    d.focus = d.focus_after_switch(sid);
    d.tasks[0].status = TaskStatus::Done;
    d.focus = d.focus_after_switch(wid);
    assert_eq!(d.focus.current_task_id, None);
}
