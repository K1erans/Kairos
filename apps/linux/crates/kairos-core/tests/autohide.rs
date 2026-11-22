use kairos_core::dock::*;

/// The Swift tests' `Date(timeIntervalSince1970: 1_000_000)`, as `Date` stores it (seconds since
/// 2001). The same base keeps `(t + 0.2) - t` from rounding below 0.2, as it does on the Mac.
const T0: f64 = 1_000_000.0 - 978_307_200.0;

fn away() -> HideInputs {
    HideInputs::default()
}
fn edge() -> HideInputs {
    HideInputs { at_edge: true, ..Default::default() }
}
fn on_bar() -> HideInputs {
    HideInputs { on_bar: true, ..Default::default() }
}
fn held() -> HideInputs {
    HideInputs { held: true, ..Default::default() }
}
fn push() -> HideInputs {
    HideInputs { at_edge: true, pushed: true, ..Default::default() }
}
fn stride(from: f64, to: f64, by: f64) -> Vec<f64> {
    let mut out = vec![];
    let mut x = from;
    while x <= to + 1e-9 {
        out.push(x);
        x += by;
    }
    out
}

/// Runs the machine as the window does: an update at every decision time.
fn settle(a: &mut DockAutoHide, inputs: HideInputs, start: f64, seconds: f64) {
    let mut now = start;
    a.update(inputs, None, now);
    while let Some(next) = a.next_decision() {
        if next > start + seconds || next <= now {
            break;
        }
        now = next;
        a.update(inputs, None, now);
    }
}

fn hidden() -> DockAutoHide {
    let mut a = DockAutoHide::new(HideRule::AutoHide, T0);
    settle(&mut a, away(), T0, 5.0);
    a
}

// MARK: AutoHideTests

#[test]
fn it_hides_shortly_after_the_pointer_leaves() {
    let mut a = DockAutoHide::new(HideRule::AutoHide, T0);
    a.update(away(), None, T0 + 0.4);
    assert_eq!(a.state, HideState::Shown { left_at: Some(T0) }, "still out inside the hide delay");
    a.update(away(), None, T0 + HIDE_DELAY);
    assert_eq!(a.state, HideState::Hiding { from: 1.0, at: T0 + HIDE_DELAY });
    assert!(!a.is_out(), "it stops taking clicks as it leaves");
    a.update(away(), None, T0 + HIDE_DELAY + SLIDE);
    assert_eq!(a.state, HideState::Hidden { armed_at: None });
    assert_eq!(a.visible(T0 + 9.0), 0.0);
}

#[test]
fn it_stays_out_while_the_pointer_is_on_it_or_something_holds_it() {
    let mut a = DockAutoHide::new(HideRule::AutoHide, T0);
    for s in stride(0.0, 10.0, 0.25) {
        a.update(on_bar(), None, T0 + s);
    }
    assert_eq!(a.state, HideState::Shown { left_at: None });
    for s in stride(10.0, 20.0, 0.25) {
        a.update(held(), None, T0 + s);
    }
    assert_eq!(a.state, HideState::Shown { left_at: None });
    a.update(away(), None, T0 + 20.25);
    assert_eq!(a.state, HideState::Shown { left_at: Some(T0 + 20.25) }, "the hide delay starts when the menu closes");
}

#[test]
fn touching_the_edge_reveals_after_a_brief_delay() {
    let mut a = hidden();
    let t = T0 + 10.0;
    a.update(edge(), None, t);
    assert_eq!(a.state, HideState::Hidden { armed_at: Some(t) });
    assert_eq!(a.next_decision(), Some(t + REVEAL_DELAY), "the window wakes even if the pointer rests");
    a.update(edge(), None, t + REVEAL_DELAY / 2.0);
    assert_eq!(a.state, HideState::Hidden { armed_at: Some(t) });
    a.update(edge(), None, t + REVEAL_DELAY);
    assert_eq!(a.state, HideState::Revealing { from: 0.0, at: t + REVEAL_DELAY });
    assert!(a.is_out());
    a.update(on_bar(), None, t + REVEAL_DELAY + SLIDE);
    assert_eq!(a.state, HideState::Shown { left_at: None });
}

#[test]
fn brushing_past_the_edge_does_not_reveal() {
    let mut a = hidden();
    a.update(edge(), None, T0 + 10.0);
    a.update(away(), None, T0 + 10.1);
    a.update(edge(), None, T0 + 10.15);
    a.update(edge(), None, T0 + 10.25);
    assert_eq!(a.state, HideState::Hidden { armed_at: Some(T0 + 10.15) }, "the delay restarts after leaving the strip");
    assert_eq!(a.visible(T0 + 10.25), 0.0);
}

#[test]
fn held_alone_never_calls_a_hidden_bar_back() {
    let mut a = hidden();
    a.update(held(), None, T0 + 10.0);
    a.update(held(), None, T0 + 11.0);
    assert_eq!(a.state, HideState::Hidden { armed_at: None });
}

#[test]
fn returning_mid_slide_reverses_without_a_jump() {
    let mut a = DockAutoHide::new(HideRule::AutoHide, T0);
    let hide_at = T0 + HIDE_DELAY;
    a.update(away(), None, hide_at);
    let mid = hide_at + SLIDE * 0.4;
    let before = a.visible(mid);
    a.update(on_bar(), None, mid);
    let HideState::Revealing { from, at } = a.state else { panic!("expected revealing, got {:?}", a.state) };
    assert_eq!(at, mid);
    assert!((from - 0.6).abs() < 1e-6);
    assert!((a.visible(mid) - before).abs() < 1e-6, "no jump");
    assert!((a.next_decision().unwrap() - mid - SLIDE * 0.4).abs() < 1e-6, "only the rest of the way");
}

#[test]
fn the_slide_is_eased_and_monotonic() {
    let mut a = hidden();
    a.update(edge(), None, T0 + 10.0);
    a.update(edge(), None, T0 + 10.2);
    let start = T0 + 10.2;
    let samples: Vec<f64> = stride(0.0, SLIDE, 0.01).iter().map(|s| a.visible(start + s)).collect();
    assert!(samples.windows(2).all(|w| w[0] <= w[1]));
    assert!(samples[3] < 0.1, "eases in: slow at the start");
    assert!(samples[samples.len() - 4] > 0.9, "eases out: slow at the end");
    assert!((samples.last().unwrap() - 1.0).abs() < 1e-9);
}

// MARK: FullScreenBarTests

fn run(a: &mut DockAutoHide, steps: &[(f64, HideInputs)], rule: HideRule) {
    for (t, inputs) in steps {
        a.update(*inputs, Some(rule), T0 + t);
    }
}

fn idle(a: &mut DockAutoHide, rule: HideRule, start: f64, seconds: f64) {
    let mut now = T0 + start;
    a.update(away(), Some(rule), now);
    while let Some(next) = a.next_decision() {
        if next > T0 + start + seconds || next <= now {
            break;
        }
        now = next;
        a.update(away(), Some(rule), now);
    }
    a.update(away(), Some(rule), T0 + start + seconds);
}

#[test]
fn entering_a_full_screen_window_takes_the_bar_away_at_once() {
    for start in [HideRule::Pinned, HideRule::AutoHide] {
        let mut a = DockAutoHide::new(start, T0);
        run(&mut a, &[(0.0, on_bar()), (1.0, away())], start);
        a.update(away(), Some(HideRule::FullScreen), T0 + 1.1);
        assert_eq!(a.state, HideState::Hidden { armed_at: None }, "from {start:?}: gone at once, no slide");
        assert_eq!(a.visible(T0 + 1.1), 0.0);
    }
}

#[test]
fn pinned_hides_in_full_screen_and_is_back_on_the_desktop() {
    let mut a = DockAutoHide::new(HideRule::Pinned, T0);
    a.update(away(), Some(HideRule::FullScreen), T0 + 1.0);
    assert_eq!(a.visible(T0 + 1.0), 0.0);
    a.update(away(), Some(HideRule::Pinned), T0 + 2.0);
    assert_eq!(a.state, HideState::Shown { left_at: None });
    assert_eq!(a.visible(T0 + 2.0), 1.0);
    assert_eq!(a.next_decision(), None, "pinned, nothing to wake for");
}

#[test]
fn resting_at_the_edge_does_not_reveal_in_full_screen() {
    let mut a = DockAutoHide::new(HideRule::FullScreen, T0);
    run(&mut a, &[(1.0, push()), (1.2, edge()), (2.0, edge()), (5.0, edge())], HideRule::FullScreen);
    assert_eq!(a.state, HideState::Hidden { armed_at: Some(T0 + 1.0) });
    assert_eq!(a.next_decision(), None, "only a push brings it out, and a push is an event");
}

#[test]
fn one_long_shove_does_not_reveal_in_full_screen() {
    let mut a = DockAutoHide::new(HideRule::FullScreen, T0);
    let shove: Vec<(f64, HideInputs)> = stride(1.0, 2.0, 0.01).into_iter().map(|t| (t, push())).collect();
    run(&mut a, &shove, HideRule::FullScreen);
    assert_eq!(a.visible(T0 + 2.0), 0.0, "a push that keeps going is still the first push");
}

#[test]
fn a_second_push_reveals_in_full_screen() {
    let mut a = DockAutoHide::new(HideRule::FullScreen, T0);
    run(&mut a, &[(1.0, push()), (1.05, push()), (1.1, push())], HideRule::FullScreen);
    assert_eq!(a.visible(T0 + 1.1), 0.0);
    let again = T0 + 1.1 + SECOND_PUSH_PAUSE + 0.01;
    a.update(push(), Some(HideRule::FullScreen), again);
    assert_eq!(a.state, HideState::Revealing { from: 0.0, at: again });
    assert!(a.is_out());
}

#[test]
fn leaving_the_edge_between_pushes_starts_over() {
    let mut a = DockAutoHide::new(HideRule::FullScreen, T0);
    run(&mut a, &[(1.0, push()), (1.5, away()), (1.6, push())], HideRule::FullScreen);
    assert_eq!(a.state, HideState::Hidden { armed_at: Some(T0 + 1.6) }, "arriving again is a first push");
}

#[test]
fn revealed_in_full_screen_it_hides_when_the_pointer_leaves() {
    let mut a = DockAutoHide::new(HideRule::FullScreen, T0);
    run(&mut a, &[(1.0, push()), (2.0, push())], HideRule::FullScreen);
    run(&mut a, &[(2.0 + SLIDE, on_bar()), (3.0, on_bar())], HideRule::FullScreen);
    assert_eq!(a.state, HideState::Shown { left_at: None });
    run(&mut a, &[(4.0, away()), (4.0 + HIDE_DELAY, away())], HideRule::FullScreen);
    assert_eq!(a.state, HideState::Hiding { from: 1.0, at: T0 + 4.0 + HIDE_DELAY });
}

#[test]
fn the_bar_in_use_stays_when_a_full_screen_window_comes_forward() {
    let mut a = DockAutoHide::new(HideRule::Pinned, T0);
    a.update(on_bar(), Some(HideRule::FullScreen), T0 + 1.0);
    assert_eq!(a.state, HideState::Shown { left_at: None }, "not snatched from under the pointer");
}

#[test]
fn a_workspace_switch_leaves_the_bar_as_it_was() {
    let mut shown = DockAutoHide::new(HideRule::AutoHide, T0);
    run(&mut shown, &[(0.0, on_bar()), (1.0, on_bar())], HideRule::AutoHide);
    let before = shown;
    shown.update(on_bar(), Some(HideRule::AutoHide), T0 + 1.5);
    assert_eq!(shown, before);
    let mut hidden = DockAutoHide::new(HideRule::AutoHide, T0);
    run(&mut hidden, &[(0.0, away()), (0.5, away()), (0.8, away())], HideRule::AutoHide);
    assert_eq!(hidden.state, HideState::Hidden { armed_at: None });
    hidden.update(away(), Some(HideRule::AutoHide), T0 + 3.0);
    assert_eq!(hidden.state, HideState::Hidden { armed_at: None });
    let mut pinned = DockAutoHide::new(HideRule::Pinned, T0);
    for t in stride(0.0, 3.0, 0.5) {
        pinned.update(away(), Some(HideRule::Pinned), T0 + t);
    }
    assert_eq!(pinned.visible(T0 + 3.0), 1.0);
}

#[test]
fn between_auto_hide_and_full_screen_a_hidden_bar_stays_hidden() {
    let mut a = DockAutoHide::new(HideRule::FullScreen, T0);
    a.update(away(), Some(HideRule::AutoHide), T0 + 1.0);
    assert_eq!(a.state, HideState::Hidden { armed_at: None }, "back on a desktop with auto-hide, it does not pop out");
}

#[test]
fn the_rule_follows_both_settings_and_full_screen() {
    assert_eq!(HideRule::new(false, true, false), HideRule::Pinned);
    assert_eq!(HideRule::new(false, true, true), HideRule::FullScreen);
    assert_eq!(HideRule::new(false, false, false), HideRule::Pinned);
    assert_eq!(HideRule::new(false, false, true), HideRule::Pinned);
    for hides in [true, false] {
        assert_eq!(HideRule::new(true, hides, false), HideRule::AutoHide);
        assert_eq!(HideRule::new(true, hides, true), HideRule::FullScreen);
    }
}

#[test]
fn pinned_with_full_screen_hiding_stays_on_an_empty_desktop_and_hides_in_full_screen() {
    let rule = |fs: bool| HideRule::new(false, true, fs);
    let mut a = DockAutoHide::new(rule(false), T0);
    idle(&mut a, rule(false), 0.0, 60.0);
    assert_eq!(a.visible(T0 + 60.0), 1.0, "a minute on an empty desktop with the pointer away: still out");
    idle(&mut a, rule(true), 61.0, 10.0);
    assert_eq!(a.visible(T0 + 71.0), 0.0, "gone in the full-screen window");
    run(&mut a, &[(72.0, push()), (72.5, push())], rule(true));
    assert!(a.is_out(), "a second push brings it out there");
    a.update(away(), Some(rule(false)), T0 + 73.0);
    assert_eq!(a.state, HideState::Shown { left_at: None });
    assert_eq!(a.visible(T0 + 73.0), 1.0, "back on the desktop at once, without touching the edge");
}

#[test]
fn returning_from_full_screen_is_a_snap_not_a_slide() {
    let mut a = DockAutoHide::new(HideRule::FullScreen, T0);
    a.update(away(), Some(HideRule::Pinned), T0 + 1.0);
    assert!(!a.is_sliding());
    assert_eq!(a.next_decision(), None, "nothing animates after the switch lands");
}

#[test]
fn both_off_keeps_the_bar_over_full_screen_windows() {
    let rule = HideRule::new(false, false, true);
    let mut a = DockAutoHide::new(HideRule::Pinned, T0);
    idle(&mut a, rule, 0.0, 30.0);
    assert_eq!(a.visible(T0 + 30.0), 1.0);
}

#[test]
fn auto_hide_with_full_screen_hiding_off_still_keeps_out_of_full_screen() {
    let desktop = HideRule::new(true, false, false);
    let full = HideRule::new(true, false, true);
    let mut a = DockAutoHide::new(desktop, T0);
    idle(&mut a, desktop, 0.0, 5.0);
    assert_eq!(a.visible(T0 + 5.0), 0.0, "auto-hide as before on a desktop");
    run(&mut a, &[(6.0, edge()), (6.0 + REVEAL_DELAY, edge())], desktop);
    assert!(a.is_out(), "resting on the edge brings it out on a desktop");
    a.update(away(), Some(full), T0 + 7.0);
    assert_eq!(a.visible(T0 + 7.0), 0.0);
    run(&mut a, &[(8.0, edge()), (9.0, edge())], full);
    assert!(!a.is_out(), "resting is not enough in full screen");
    run(&mut a, &[(9.5, push())], full);
    assert!(a.is_out(), "the second push is");
}
