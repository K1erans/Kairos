mod common;
use common::*;
use kairos_core::tempo::{menu_title, StrictConfirmation, TempoEngine, TempoEvent, TempoPhase};
use kairos_core::{Date, Mode, Settings, TempoPreset};

const T0: f64 = 1_000_000.0;
fn t0() -> Date {
    t(T0)
}
fn at(s: f64) -> Date {
    t(T0 + s)
}
fn preset(cycles: i32) -> TempoPreset {
    let mut p = TempoPreset::new("C", 25, 5, 15, false);
    p.cycles_before_long = cycles;
    p
}
fn classic() -> TempoPreset {
    TempoPreset::defaults().remove(0)
}
fn finished(events: &[TempoEvent]) -> Vec<TempoPhase> {
    events.iter().filter_map(|e| if let TempoEvent::Finished { phase, .. } = e { Some(*phase) } else { None }).collect()
}

// MARK: TempoTests

#[test]
fn full_cycle_and_long_break() {
    let mut e = TempoEngine::new(TempoPreset::new("T", 25, 5, 15, false).with_cycles(2));
    e.start(TempoPhase::Focus, false, t0());
    assert_eq!(e.phase(), TempoPhase::Focus);
    assert_eq!(e.remaining(at(60.0)), 24.0 * 60.0);
    let events = e.tick(at(25.0 * 60.0), true);
    assert_eq!(e.phase(), TempoPhase::ShortBreak);
    assert_eq!(
        events.first(),
        Some(&TempoEvent::Finished { phase: TempoPhase::Focus, start: t0(), end: at(1500.0), duration: 1500.0, interruptions: 0, completed: true })
    );
    e.tick(at(30.0 * 60.0), true);
    assert_eq!(e.phase(), TempoPhase::Focus);
    e.tick(at(55.0 * 60.0), true);
    assert_eq!(e.phase(), TempoPhase::LongBreak);
}

trait WithCycles {
    fn with_cycles(self, n: i32) -> Self;
}
impl WithCycles for TempoPreset {
    fn with_cycles(mut self, n: i32) -> Self {
        self.cycles_before_long = n;
        self
    }
}

#[test]
fn sleep_past_the_end_still_ends_on_time() {
    let mut e = TempoEngine::new(classic());
    e.start(TempoPhase::Focus, false, t0());
    let events = e.tick(at(3.0 * 3600.0), true);
    let Some(TempoEvent::Finished { end, duration, completed, .. }) = events.first() else { panic!("{events:?}") };
    assert_eq!(*end, at(1500.0));
    assert_eq!(*duration, 1500.0);
    assert!(*completed);
}

#[test]
fn pause_resume_extend_and_skip() {
    let mut e = TempoEngine::new(classic());
    e.start(TempoPhase::Focus, false, t0());
    e.pause(at(600.0));
    assert_eq!(e.remaining(at(9999.0)), 900.0);
    assert!(e.tick(at(9999.0), true).is_empty());
    e.resume(at(1000.0));
    assert_eq!(e.ends_at(), Some(at(1900.0)));
    e.extend(5, at(1000.0));
    assert_eq!(e.ends_at(), Some(at(2200.0)));
    assert_eq!(e.interruptions(), 1);
    let events = e.skip(at(1100.0));
    assert_eq!(
        events.first(),
        Some(&TempoEvent::Finished { phase: TempoPhase::Focus, start: t0(), end: at(1100.0), duration: 700.0, interruptions: 2, completed: false })
    );
    assert_eq!(e.phase(), TempoPhase::ShortBreak);
}

// MARK: TempoSleepTests

#[test]
fn a_long_sleep_logs_only_the_phase_that_ran_and_then_goes_idle() {
    let mut e = TempoEngine::new(preset(2));
    e.start(TempoPhase::Focus, false, t0());
    let wake = 3.0 * 3600.0;
    let mut logged = finished(&e.tick(at(wake), true));
    for s in 1..=120 {
        logged.extend(finished(&e.tick(at(wake + s as f64), true)));
    }
    assert_eq!(logged, vec![TempoPhase::Focus], "no phantom sessions after waking");
    assert_eq!(e.phase(), TempoPhase::Idle);
}

#[test]
fn waking_inside_the_next_phase_lands_in_it() {
    let mut e = TempoEngine::new(preset(2));
    e.start(TempoPhase::Focus, false, t0());
    let wake = 25.0 * 60.0 + 120.0;
    let events = e.tick(at(wake), true);
    assert_eq!(finished(&events), vec![TempoPhase::Focus]);
    assert_eq!(e.phase(), TempoPhase::ShortBreak);
    assert_eq!(e.remaining(at(wake)), 180.0);
    assert!(e.tick(at(wake + 1.0), true).is_empty());
}

#[test]
fn stop_forgets_the_cycle_count() {
    let mut e = TempoEngine::new(preset(2));
    e.start(TempoPhase::Focus, false, t0());
    e.tick(at(1500.0), true);
    e.stop(at(1600.0));
    e.start(TempoPhase::Focus, false, at(2000.0));
    e.tick(at(3500.0), true);
    assert_eq!(e.phase(), TempoPhase::ShortBreak, "a new timer starts a new cycle");
}

#[test]
fn pausing_at_the_end_still_completes() {
    let mut e = TempoEngine::new(preset(2));
    e.start(TempoPhase::Focus, false, t0());
    e.pause(at(1500.0));
    let events = e.tick(at(1501.0), true);
    assert_eq!(finished(&events), vec![TempoPhase::Focus]);
    assert_eq!(e.phase(), TempoPhase::ShortBreak);
}

// MARK: TempoModeTests

fn relaxed() -> TempoPreset {
    TempoPreset::new("R", 25, 5, 15, false)
}
fn deep() -> TempoPreset {
    TempoPreset::new("D", 50, 10, 30, true)
}

#[test]
fn strict_chosen_by_hand_holds_for_the_whole_timer() {
    let mut e = TempoEngine::new(relaxed());
    e.start(TempoPhase::Focus, true, t0());
    assert_eq!(e.mode(), Mode::Strict);
    e.tick(at(1500.0), true);
    assert_eq!(e.mode(), Mode::OnBreak);
    e.tick(at(1800.0), true);
    assert_eq!(e.phase(), TempoPhase::Focus);
    assert_eq!(e.mode(), Mode::Strict, "the next focus phase is strict too");
    e.stop(at(1900.0));
    assert_eq!(e.mode(), Mode::Off);
    e.start(TempoPhase::Focus, false, at(2000.0));
    assert_eq!(e.mode(), Mode::Focus, "a new timer does not inherit it");
}

#[test]
fn a_strict_preset_makes_focus_strict() {
    let mut e = TempoEngine::new(deep());
    e.start(TempoPhase::Focus, false, t0());
    assert_eq!(e.mode(), Mode::Strict);
}

#[test]
fn a_new_preset_mid_phase_applies_from_the_next_phase() {
    let mut e = TempoEngine::new(relaxed());
    e.start(TempoPhase::Focus, false, t0());
    e.use_preset(deep());
    assert_eq!(e.ends_at(), Some(at(1500.0)), "the running phase keeps its length");
    assert_eq!(e.mode(), Mode::Focus, "and its mode");
    assert!(e.next_focus_is_strict());
    e.tick(at(1500.0), true);
    assert_eq!(e.remaining(at(1500.0)), 600.0, "the break is the new preset's");
    e.tick(at(2100.0), true);
    assert_eq!(e.mode(), Mode::Strict);
    assert_eq!(e.remaining(at(2100.0)), 3000.0);
}

#[test]
fn a_preset_chosen_while_idle_applies_at_once() {
    let mut e = TempoEngine::new(relaxed());
    e.use_preset(deep());
    e.start(TempoPhase::Focus, false, t0());
    assert_eq!(e.remaining(t0()), 3000.0);
}

// MARK: BreakWaitTests

fn engine_waiting_after_first_break() -> (TempoEngine, Vec<TempoEvent>) {
    let mut e = TempoEngine::new(preset(2));
    e.start(TempoPhase::Focus, false, t0());
    e.tick(at(25.0 * 60.0), false);
    let events = e.tick(at(30.0 * 60.0), false);
    (e, events)
}

#[test]
fn the_setting_is_off_by_default() {
    assert!(!Settings::default().starts_focus_after_break);
}

#[test]
fn a_break_that_runs_out_waits_instead_of_starting_focus() {
    let (mut e, events) = engine_waiting_after_first_break();
    assert_eq!(e.phase(), TempoPhase::Idle);
    assert_eq!(e.mode(), Mode::Off);
    assert!(e.awaiting_focus());
    assert_eq!(events.last(), Some(&TempoEvent::AwaitingFocus));
    let Some(TempoEvent::Finished { phase, completed, .. }) = events.first() else { panic!("the break is logged") };
    assert_eq!(*phase, TempoPhase::ShortBreak);
    assert!(*completed);
    assert!(e.tick(at(60.0 * 60.0), false).is_empty(), "nothing runs while waiting");
}

#[test]
fn focus_running_out_still_goes_straight_to_the_break() {
    let mut e = TempoEngine::new(preset(2));
    e.start(TempoPhase::Focus, false, t0());
    e.tick(at(25.0 * 60.0), false);
    assert_eq!(e.phase(), TempoPhase::ShortBreak);
    assert!(!e.awaiting_focus());
}

#[test]
fn starting_after_the_wait_keeps_the_count_for_the_long_break() {
    let (mut e, _) = engine_waiting_after_first_break();
    let start = 40.0 * 60.0;
    e.start(TempoPhase::Focus, false, at(start));
    assert!(!e.awaiting_focus());
    e.tick(at(start + 25.0 * 60.0), false);
    assert_eq!(e.phase(), TempoPhase::LongBreak, "the second focus of two before a long break");
}

#[test]
fn stopping_the_wait_starts_a_new_count() {
    let (mut e, _) = engine_waiting_after_first_break();
    assert_eq!(e.stop(at(35.0 * 60.0)), vec![TempoEvent::Stopped]);
    assert!(!e.awaiting_focus());
    let start = 40.0 * 60.0;
    e.start(TempoPhase::Focus, false, at(start));
    e.tick(at(start + 25.0 * 60.0), false);
    assert_eq!(e.phase(), TempoPhase::ShortBreak);
}

#[test]
fn not_now_ends_the_wait_and_the_count() {
    let (mut e, _) = engine_waiting_after_first_break();
    e.end_wait();
    assert!(!e.awaiting_focus());
    assert_eq!(e.phase(), TempoPhase::Idle);
    let start = 40.0 * 60.0;
    e.start(TempoPhase::Focus, false, at(start));
    e.tick(at(start + 25.0 * 60.0), false);
    assert_eq!(e.phase(), TempoPhase::ShortBreak, "a new count, so a short break");
}

#[test]
fn skipping_a_break_still_starts_focus_at_once() {
    let mut e = TempoEngine::new(preset(2));
    e.start(TempoPhase::Focus, false, t0());
    e.tick(at(25.0 * 60.0), false);
    e.skip(at(26.0 * 60.0));
    assert_eq!(e.phase(), TempoPhase::Focus);
    assert!(!e.awaiting_focus());
}

#[test]
fn with_the_setting_on_a_break_runs_straight_into_focus() {
    let mut e = TempoEngine::new(preset(2));
    e.start(TempoPhase::Focus, false, t0());
    e.tick(at(25.0 * 60.0), true);
    let events = e.tick(at(30.0 * 60.0), true);
    assert_eq!(e.phase(), TempoPhase::Focus);
    assert_eq!(events.last(), Some(&TempoEvent::Started(TempoPhase::Focus)));
}

#[test]
fn a_sleep_past_the_next_focus_still_just_stops() {
    let mut e = TempoEngine::new(preset(2));
    e.start(TempoPhase::ShortBreak, false, t0());
    let events = e.tick(at(5.0 * 3600.0), false);
    assert_eq!(events.last(), Some(&TempoEvent::Stopped));
    assert!(!e.awaiting_focus(), "hours later there is no break to come back from");
}

#[test]
fn a_wait_then_a_strict_start_holds_strict() {
    let (mut e, _) = engine_waiting_after_first_break();
    e.start(TempoPhase::Focus, true, at(40.0 * 60.0));
    assert_eq!(e.mode(), Mode::Strict);
}

// MARK: MenuBarTitleTests (the tray label on Linux)

#[test]
fn a_short_name_is_shown_whole() {
    assert_eq!(menu_title::make("Work", TempoPhase::Idle, 0.0, false), "Work");
}

#[test]
fn a_long_name_is_cut_with_an_ellipsis() {
    let name = "Write the quarterly report for the board";
    let idle = menu_title::make(name, TempoPhase::Idle, 0.0, false);
    assert_eq!(idle, "Write the quarterly…");
    assert_eq!(idle.chars().count(), menu_title::IDLE_LIMIT);
    assert_eq!(menu_title::make(name, TempoPhase::Focus, 754.0, false), "Write the quart… · 12:34");
}

#[test]
fn an_ellipsis_never_follows_a_space() {
    assert_eq!(menu_title::make("Read the books on the shelf", TempoPhase::Focus, 60.0, false), "Read the books… · 1:00");
}

#[test]
fn a_break_shows_only_the_clock() {
    assert_eq!(menu_title::make("Write the quarterly report", TempoPhase::ShortBreak, 299.5, false), "Break · 5:00");
}

#[test]
fn waiting_after_a_break_says_so() {
    assert_eq!(menu_title::make("Work", TempoPhase::Idle, 0.0, true), "Break over");
}

#[test]
fn emoji_count_as_one_character() {
    let name = "🍅".repeat(30);
    assert_eq!(menu_title::make(&name, TempoPhase::Idle, 0.0, false).chars().count(), menu_title::IDLE_LIMIT);
}

// MARK: Strict confirmation

#[test]
fn the_confirmation_names_the_end_and_says_there_is_no_way_out() {
    let ends = local(2026, 11, 9, 15, 25);
    let c = StrictConfirmation::new(50, ends);
    assert!(c.title.contains("15:25"), "{}", c.title);
    assert!(c.message.contains("stopped, skipped or quit"), "{}", c.message);
    assert!(c.message.contains("50 minutes"), "{}", c.message);
}
