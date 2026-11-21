mod common;
use common::*;
use kairos_core::quickadd::{parse, suggest, ParsedTask};
use kairos_core::{Id, TaskItem};

// Monday 9 Nov 2026, 10:00 local.
fn now() -> kairos_core::Date {
    local(2026, 11, 9, 10, 0)
}
fn groups() -> Vec<String> {
    vec!["Work".into(), "Study".into()]
}
fn p(s: &str) -> ParsedTask {
    parse(s, &groups(), now())
}
fn day(d: u32, h: u32, m: u32) -> Option<kairos_core::Date> {
    Some(local(2026, 11, d, h, m))
}

#[test]
fn short_weekdays_are_words_inside_the_title() {
    assert_eq!(p("Buy sun hat").title, "Buy sun hat");
    assert_eq!(p("Buy sun hat").due, None);
    let sat = p("Research sat scores friday");
    assert_eq!(sat.title, "Research sat scores");
    assert_eq!(sat.due, day(13, 17, 0), "full weekday names always count");
}

#[test]
fn short_weekdays_count_at_the_end_or_before_a_time() {
    assert_eq!(p("Call mom sun").due, day(15, 17, 0));
    assert_eq!(p("Call mom sun").title, "Call mom");
    assert_eq!(p("Call mom sun @Work !high").due, day(15, 17, 0), "only tokens follow it");
    assert_eq!(p("Gym sat 9am then swim").due, day(14, 9, 0));
    assert_eq!(p("Gym sat 9am then swim").title, "Gym then swim");
}

#[test]
fn duration_spellings() {
    assert_eq!(p("a ~45min").estimate_min, Some(45));
    assert_eq!(p("a ~45mins").estimate_min, Some(45));
    assert_eq!(p("a ~2hr").estimate_min, Some(120));
    assert_eq!(p("a ~2hrs").estimate_min, Some(120));
    assert_eq!(p("a ~1h30").estimate_min, Some(90));
    assert_eq!(p("a ~1h30m").estimate_min, Some(90));
    assert_eq!(p("a ~90").estimate_min, Some(90));
    assert_eq!(p("a ~h").estimate_min, None);
    assert_eq!(p("a ~").estimate_min, None);
    assert_eq!(p("a ~soon").title, "a ~soon");
}

#[test]
fn unknown_group_stays_in_the_title() {
    let r = p("Plan trip @Holiday");
    assert_eq!(r.title, "Plan trip @Holiday");
    assert_eq!(r.group_name, None);
    assert_eq!(p("Plan trip @work").group_name.as_deref(), Some("Work"), "known groups match in any case");
}

#[test]
fn parses_all_five_tokens() {
    let r = p("Draft blog post tomorrow 3pm @Work ~45m !high");
    assert_eq!(r.title, "Draft blog post");
    assert_eq!(r.group_name.as_deref(), Some("Work"));
    assert_eq!(r.estimate_min, Some(45));
    assert_eq!(r.priority, 3);
    assert_eq!(r.due, day(10, 15, 0));
}

#[test]
fn plain_title_keeps_everything() {
    let r = p("Buy 3 apples");
    assert_eq!(r.title, "Buy 3 apples");
    assert_eq!(r.due, None);
    assert_eq!(r.estimate_min, None);
    assert_eq!(r.priority, 0);
}

#[test]
fn weekday_and_durations() {
    let r = p("Review fri 9:30am ~1h30m #Study !low");
    assert_eq!(r.title, "Review");
    assert_eq!(r.estimate_min, Some(90));
    assert_eq!(r.group_name.as_deref(), Some("Study"));
    assert_eq!(r.priority, 1);
    assert_eq!(r.due, day(13, 9, 30));
}

#[test]
fn bare_time_in_the_past_rolls_to_tomorrow() {
    assert_eq!(p("Call 8am").due, day(10, 8, 0));
}

#[test]
fn same_weekday_means_next_week() {
    assert_eq!(p("Plan monday").due, day(16, 17, 0));
}

#[test]
fn smart_next_prefers_due_then_priority() {
    let g = Id::new();
    let mut a = TaskItem::new("a", Some(g));
    a.priority = 3;
    let mut b = TaskItem::new("b", Some(g));
    b.due = Some(t(10.0));
    let mut c = TaskItem::new("c", Some(g));
    c.due = Some(t(5.0));
    c.estimate_min = Some(120);
    let tasks = vec![a.clone(), b, c];
    assert_eq!(suggest(&tasks, Some(g), None, None).map(|t| t.title), Some("c".into()));
    assert_eq!(suggest(&tasks, Some(g), None, Some(30)).map(|t| t.title), Some("b".into()));
    assert_eq!(suggest(&[a], Some(Id::new()), None, None), None);
}

#[test]
fn an_estimate_too_big_for_minutes_is_ignored() {
    use kairos_core::quickadd::minutes;
    assert_eq!(minutes("3000000000m"), None, "used to wrap to a negative number");
    assert_eq!(minutes("3000000000"), None);
    assert_eq!(minutes("999999999999999999h"), None, "used to overflow and panic");
    assert_eq!(minutes("99999999999999999999h"), None);
    assert_eq!(minutes("35791394h8m"), None, "past i32::MAX by one minute");
    assert_eq!(minutes("35791394h7m"), Some(i32::MAX));
    let p = parse("write report ~3000000000m", &groups(), now());
    assert_eq!(p.estimate_min, None);
    assert_eq!(p.title, "write report ~3000000000m", "the word stays in the title, as other words that aren't tokens do");
}
