mod common;
use common::*;
use chrono::{TimeZone, Utc};
use kairos_core::clearance::{self, Rect, Window};
use kairos_core::duetext;
use kairos_core::hotkeys::{self, Action, HotkeyPlan};
use kairos_core::reminders::{self, Inputs};
use kairos_core::setup::{SetupChecklist, SetupFacts, SetupStep};
use kairos_core::*;
use std::collections::HashSet;

// MARK: SetupChecklistTests (Linux has no Accessibility step)

#[test]
fn a_new_machine_has_everything_left_to_do() {
    let list = SetupChecklist::new(&SetupFacts::default());
    assert_eq!(
        list.items.iter().map(|i| i.step).collect::<Vec<_>>(),
        vec![SetupStep::Shield, SetupStep::Read, SetupStep::JevKey, SetupStep::Scenes, SetupStep::OpenAtLogin, SetupStep::DockReplacement]
    );
    assert_eq!(list.done_count(), 0);
    assert!(!list.is_complete());
}

#[test]
fn a_voice_key_is_asked_for_only_when_a_cloud_voice_is_chosen() {
    let mut facts = SetupFacts::default();
    assert!(!SetupChecklist::new(&facts).items.iter().any(|i| i.step == SetupStep::VoiceKey));
    facts.voice_provider = VoiceProvider::Elevenlabs;
    let item = SetupChecklist::new(&facts).items.into_iter().find(|i| i.step == SetupStep::VoiceKey).unwrap();
    assert_eq!(item.title, "Add your ElevenLabs key");
    assert!(!item.done);
    facts.has_voice_key = true;
    assert!(SetupChecklist::new(&facts).items.iter().find(|i| i.step == SetupStep::VoiceKey).unwrap().done);
}

#[test]
fn an_extension_counts_once_it_has_connected() {
    let mut facts = SetupFacts::default();
    facts.seen_clients = HashSet::from(["shield".to_string()]);
    let list = SetupChecklist::new(&facts);
    assert!(list.items.iter().find(|i| i.step == SetupStep::Shield).unwrap().done);
    assert!(!list.items.iter().find(|i| i.step == SetupStep::Read).unwrap().done);
}

#[test]
fn everything_done_completes_the_list() {
    let mut f = SetupFacts::default();
    f.seen_clients = HashSet::from(["shield".to_string(), "read".to_string()]);
    f.has_jev_key = true;
    f.has_node = true;
    f.has_scene_helper = true;
    f.voice_provider = VoiceProvider::Openai;
    f.has_voice_key = true;
    f.open_at_login = true;
    f.replaces_dock = true;
    let list = SetupChecklist::new(&f);
    assert!(list.is_complete());
    assert_eq!(list.done_count(), 7);
}

// MARK: DueReminderTests

const NOW: f64 = 2_000_000_000.0;

fn task(title: &str, minutes: Option<f64>, done: bool, group: Option<Id>) -> TaskItem {
    let mut t = TaskItem::new(title, group);
    t.due = minutes.map(|m| common::t(NOW + m * 60.0));
    if done {
        t.status = TaskStatus::Done;
    }
    t
}
fn data(tasks: Vec<TaskItem>) -> AppData {
    let mut d = AppData::starter(None, &|_| true);
    d.tasks = tasks;
    d
}

#[test]
fn only_unfinished_tasks_due_later_are_reminded() {
    let d = data(vec![task("later", Some(30.0), false, None), task("past", Some(-5.0), false, None), task("done", Some(30.0), true, None), task("no date", None, false, None)]);
    assert_eq!(reminders::plan(&Inputs::new(&d), t(NOW)).iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), vec!["later"]);
}

#[test]
fn the_soonest_come_first_and_there_are_never_more_than_64() {
    let d = data((1..=100).rev().map(|i| task(&format!("t{i}"), Some(i as f64), false, None)).collect());
    let plan = reminders::plan(&Inputs::new(&d), t(NOW));
    assert_eq!(plan.len(), reminders::LIMIT);
    assert_eq!(plan.first().unwrap().title, "t1");
    assert_eq!(plan.last().unwrap().title, "t64");
}

#[test]
fn a_reminder_says_when_and_where() {
    let mut d = AppData::starter(None, &|_| true);
    let work = d.groups[0].clone();
    let tk = task("Send the report", Some(10.0), false, Some(work.id));
    d.tasks = vec![tk.clone()];
    let r = &reminders::plan(&Inputs::new(&d), t(NOW))[0];
    assert_eq!(Some(r.fire_at), tk.due);
    assert_eq!(r.body, format!("Due now, in {}.", work.name));
    assert_eq!(r.id, format!("{}{}", reminders::ID_PREFIX, tk.id.string()));
}

#[test]
fn turning_reminders_off_plans_none() {
    let mut d = data(vec![task("later", Some(30.0), false, None)]);
    d.settings.notifies_when_due = false;
    assert!(reminders::plan(&Inputs::new(&d), t(NOW)).is_empty());
}

// MARK: DueTextTests (UTC)

fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Date {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
}
fn text(due: Date) -> String {
    duetext::short(due, at(2026, 11, 9, 10, 0), &Utc)
}

#[test]
fn near_days_are_named() {
    assert_eq!(text(at(2026, 11, 9, 15, 0)), "Today 15:00");
    assert_eq!(text(at(2026, 11, 10, 11, 0)), "Tomorrow 11:00");
    assert_eq!(text(at(2026, 11, 8, 15, 0)), "Yesterday 15:00");
}

#[test]
fn the_coming_week_uses_the_weekday() {
    assert_eq!(text(at(2026, 11, 11, 15, 0)), "Wed 15:00");
    assert_eq!(text(at(2026, 11, 15, 15, 0)), "Sun 15:00");
}

#[test]
fn later_this_year_drops_the_year() {
    assert_eq!(text(at(2026, 11, 20, 15, 0)), "20 Nov 15:00");
    assert_eq!(text(at(2026, 11, 2, 15, 0)), "2 Nov 15:00", "last week is a date, not a weekday that reads as next week");
}

#[test]
fn another_year_drops_the_time() {
    assert_eq!(text(at(2027, 1, 5, 15, 0)), "5 Jan 2027");
}

#[test]
fn every_form_stays_short() {
    for due in [at(2026, 11, 9, 15, 0), at(2026, 11, 11, 15, 0), at(2026, 12, 24, 23, 59), at(2027, 9, 30, 11, 0)] {
        assert!(text(due).chars().count() <= 15, "{}", text(due));
    }
}

// MARK: HotkeyPlanTests (X11 keysyms; Linux defaults avoid Alt+Space, Cinnamon's window menu)

#[test]
fn a_new_profile_registers_quick_add_and_timer_but_not_the_group_keys() {
    let plan = HotkeyPlan::new(&Settings::default());
    assert_eq!(plan.entries.iter().map(|e| e.action).collect::<Vec<_>>(), vec![Action::QuickAdd, Action::Timer]);
    assert_eq!(plan.entries.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(), vec!["Ctrl+Alt+Space", "Ctrl+Alt+P"]);
}

#[test]
fn each_shortcut_can_be_changed_or_turned_off() {
    let mut s = Settings::default();
    s.quick_add_hotkey = "control-option-n".into();
    s.timer_hotkey = hotkeys::OFF.into();
    s.group_hotkeys = true;
    let plan = HotkeyPlan::new(&s);
    assert_eq!(plan.entries[0].label, "Ctrl+Alt+N");
    assert_eq!(plan.entries[0].keysym, hotkeys::XK_N);
    assert_eq!(plan.entries[0].modifiers, hotkeys::CONTROL | hotkeys::ALT);
    assert!(!plan.entries.iter().any(|e| e.action == Action::Timer));
    let groups: Vec<_> = plan.entries.iter().filter(|e| matches!(e.action, Action::Group(_))).collect();
    assert_eq!(groups.len(), 9);
    assert_eq!(groups[0].keysym, hotkeys::XK_1);
    assert_eq!(groups[8].label, "Ctrl+9");
}

#[test]
fn an_unknown_stored_shortcut_falls_back_to_the_default() {
    let mut s = Settings::default();
    s.quick_add_hotkey = "hyper-q".into();
    assert_eq!(HotkeyPlan::new(&s).entries[0].label, "Ctrl+Alt+Space");
}

#[test]
fn a_mac_id_keeps_its_shortcut_where_the_keys_exist() {
    let mut s = Settings::default();
    s.quick_add_hotkey = "option-space".into();
    s.timer_hotkey = "option-shift-space".into();
    let plan = HotkeyPlan::new(&s);
    assert_eq!(plan.entries.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(), vec!["Alt+Space", "Alt+Shift+Space"]);
}

#[test]
fn the_two_lists_never_offer_the_same_shortcut() {
    let a: HashSet<(u32, u16)> = hotkeys::QUICK_ADD.iter().map(|c| (c.keysym, c.modifiers)).collect();
    let b: HashSet<(u32, u16)> = hotkeys::TIMER.iter().map(|c| (c.keysym, c.modifiers)).collect();
    assert!(a.is_disjoint(&b));
}

#[test]
fn failures_are_named() {
    assert_eq!(HotkeyPlan::failure_text(&[]), None);
    assert!(HotkeyPlan::failure_text(&["Alt+Space".into()]).unwrap().starts_with("Alt+Space is already taken"));
    assert!(HotkeyPlan::failure_text(&["Ctrl+1".into(), "Ctrl+2".into()]).unwrap().contains("are already taken"));
}

// MARK: WindowClearanceTests (top-left origin on both platforms)

fn area() -> Rect {
    Rect::new(0.0, 34.0, 1470.0, 922.0)
}
fn bottom() -> Rect {
    clearance::strip(DockEdge::Bottom, Rect::new(438.0, 861.0, 594.0, 95.0), 6.0, 560.0, 36.0)
}
fn win(x: f64, y: f64, w: f64, h: f64) -> Window {
    Window::new(Rect::new(x, y, w, h))
}
fn clear(w: &Window) -> Option<Rect> {
    clearance::clear(w, &bottom(), DockEdge::Bottom, &area())
}

#[test]
fn the_strip_runs_along_the_bar_from_the_screen_edge() {
    assert_eq!(bottom(), Rect::new(444.0, 920.0, 560.0, 36.0));
    let side = Rect::new(0.0, 200.0, 280.0, 600.0);
    assert_eq!(clearance::strip(DockEdge::Left, side, 10.0, 500.0, 36.0), Rect::new(0.0, 210.0, 36.0, 500.0));
    let right = Rect::new(1190.0, 200.0, 280.0, 600.0);
    assert_eq!(clearance::strip(DockEdge::Right, right, 10.0, 500.0, 36.0), Rect::new(1434.0, 210.0, 36.0, 500.0));
}

#[test]
fn a_zoomed_window_is_shortened_to_end_above_the_bar() {
    assert_eq!(clear(&win(0.0, 34.0, 1470.0, 922.0)), Some(Rect::new(0.0, 34.0, 1470.0, 886.0)));
}

#[test]
fn a_window_that_fits_moves_up_keeping_its_size() {
    assert_eq!(clear(&win(300.0, 500.0, 800.0, 450.0)), Some(Rect::new(300.0, 470.0, 800.0, 450.0)));
}

#[test]
fn a_window_beside_the_bar_is_left_alone() {
    assert_eq!(clear(&win(0.0, 600.0, 400.0, 356.0)), None, "it does not reach over the bar");
    assert_eq!(clear(&win(300.0, 100.0, 800.0, 820.0)), None, "ending at the bar's edge is clear");
}

#[test]
fn full_screen_minimised_and_non_standard_windows_are_left_alone() {
    let base = win(0.0, 34.0, 1470.0, 922.0);
    for w in [Window { is_full_screen: true, ..base }, Window { is_minimized: true, ..base }, Window { is_standard: false, ..base }] {
        assert_eq!(clear(&w), None);
    }
}

#[test]
fn a_fixed_size_window_moves_if_it_fits_and_is_left_if_not() {
    let fits = Window { can_resize: false, ..win(300.0, 600.0, 500.0, 340.0) };
    let r = clear(&fits).unwrap();
    assert_eq!((r.w, r.h), (500.0, 340.0));
    let too_tall = Window { can_resize: false, ..win(300.0, 34.0, 500.0, 922.0) };
    assert_eq!(clear(&too_tall), None);
}

#[test]
fn a_window_that_cannot_move_is_only_shrunk() {
    let pinned = Window { can_move: false, ..win(300.0, 500.0, 800.0, 450.0) };
    assert_eq!(clear(&pinned), None, "it would fit by moving, which it can't, and shrinking from the top would move it too");
    let tall = Window { can_move: false, ..win(0.0, 34.0, 1470.0, 922.0) };
    assert_eq!(clear(&tall).map(|r| r.h), Some(886.0), "shrinking its foot keeps its origin");
}

#[test]
fn side_bars_push_windows_sideways() {
    let left = clearance::strip(DockEdge::Left, Rect::new(0.0, 200.0, 280.0, 600.0), 10.0, 500.0, 36.0);
    let wide = win(0.0, 34.0, 1470.0, 922.0);
    assert_eq!(clearance::clear(&wide, &left, DockEdge::Left, &area()), Some(Rect::new(36.0, 34.0, 1434.0, 922.0)));
    let small = win(10.0, 300.0, 600.0, 400.0);
    assert_eq!(clearance::clear(&small, &left, DockEdge::Left, &area()), Some(Rect::new(36.0, 300.0, 600.0, 400.0)));
    let right = clearance::strip(DockEdge::Right, Rect::new(1190.0, 200.0, 280.0, 600.0), 10.0, 500.0, 36.0);
    assert_eq!(clearance::clear(&wide, &right, DockEdge::Right, &area()), Some(Rect::new(0.0, 34.0, 1434.0, 922.0)));
    let near_right = win(1000.0, 300.0, 460.0, 400.0);
    assert_eq!(clearance::clear(&near_right, &right, DockEdge::Right, &area()), Some(Rect::new(974.0, 300.0, 460.0, 400.0)));
}

#[test]
fn a_cleared_window_is_left_alone_next_time() {
    let once = clear(&win(0.0, 34.0, 1470.0, 922.0)).unwrap();
    assert_eq!(clear(&Window::new(once)), None, "the move it causes does not set off another");
}

#[test]
fn a_sliver_is_not_worth_making() {
    let short = Window { can_move: false, ..win(300.0, 860.0, 60.0, 96.0) };
    assert_eq!(clear(&short), None);
}

#[test]
fn the_overlay_goes_to_the_monitor_showing_most_of_the_window() {
    let monitors = [(1usize, Rect::new(0.0, 0.0, 1440.0, 900.0)), (2usize, Rect::new(1440.0, -200.0, 2560.0, 1440.0))];
    assert_eq!(clearance::display_showing(&Rect::new(1300.0, 100.0, 800.0, 600.0), &monitors), Some(2));
    assert_eq!(clearance::display_showing(&Rect::new(100.0, 100.0, 800.0, 600.0), &monitors), Some(1));
    assert_eq!(clearance::display_showing(&Rect::new(-5000.0, 0.0, 10.0, 10.0), &monitors), None);
}
