//! Its own test binary, because it sets TZ for the whole process.
use chrono::{NaiveDate, TimeZone, Utc};
use kairos_core::quickadd::{parse, resolve_local};

#[test]
fn a_time_in_the_spring_forward_gap_moves_to_the_end_of_the_gap() {
    std::env::set_var("TZ", "Europe/London");
    // Saturday 27 March 2027, noon. British clocks go from 1:00 to 2:00 on Sunday the 28th.
    let now = Utc.with_ymd_and_hms(2027, 3, 27, 12, 0, 0).unwrap();
    let p = parse("call mum tomorrow 1:30am", &[], now);
    assert_eq!(p.title, "call mum");
    // 2:00 BST is 1:00 UTC, the first valid local time after the gap.
    assert_eq!(p.due, Some(Utc.with_ymd_and_hms(2027, 3, 28, 1, 0, 0).unwrap()), "the due time used to be dropped");

    let ok = parse("call mum tomorrow 3am", &[], now);
    assert_eq!(ok.due, Some(Utc.with_ymd_and_hms(2027, 3, 28, 2, 0, 0).unwrap()));

    // In autumn 1:30 happens twice; the earlier one is used.
    let autumn = Utc.with_ymd_and_hms(2027, 10, 30, 12, 0, 0).unwrap();
    let twice = parse("call mum tomorrow 1:30am", &[], autumn);
    assert_eq!(twice.due, Some(Utc.with_ymd_and_hms(2027, 10, 31, 0, 30, 0).unwrap()));
}

#[test]
fn resolve_local_works_in_any_zone() {
    let naive = NaiveDate::from_ymd_opt(2027, 3, 28).unwrap().and_hms_opt(1, 30, 0).unwrap();
    assert_eq!(resolve_local(&Utc, naive), Some(Utc.with_ymd_and_hms(2027, 3, 28, 1, 30, 0).unwrap()));
}
