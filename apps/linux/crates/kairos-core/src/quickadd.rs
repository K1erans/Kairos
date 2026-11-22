//! Parses a quick-add line such as `Draft blog post tomorrow 3pm @Work ~45m !high`.

use crate::id::Id;
use crate::models::{TaskItem, TaskStatus};
use crate::time::Date;
use chrono::{Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ParsedTask {
    pub title: String,
    pub due: Option<Date>,
    pub group_name: Option<String>,
    pub estimate_min: Option<i32>,
    pub priority: i32,
}

const WEEKDAYS: [&str; 7] = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];

enum Piece {
    Title(String),
    Group(String),
    Estimate(i32),
    Priority(i32),
    Day(NaiveDate),
    /// "sun", "sat": also plain words, so a date only where nothing but tokens follows it, or a
    /// time comes straight after.
    ShortDay(NaiveDate, String),
    Time(u32, u32),
}

impl Piece {
    fn is_title(&self) -> bool {
        matches!(self, Piece::Title(_))
    }
    fn is_time(&self) -> bool {
        matches!(self, Piece::Time(..))
    }
}

/// `groups` are the existing group names. An @word naming none of them stays in the title.
pub fn parse(input: &str, groups: &[String], now: Date) -> ParsedTask {
    let today = now.with_timezone(&Local).date_naive();
    let mut pieces = read(input, groups, today);
    let mut only_tokens_after = true;
    for i in (0..pieces.len()).rev() {
        let next_is_time = i + 1 < pieces.len() && pieces[i + 1].is_time();
        if let Piece::ShortDay(_, word) = &pieces[i] {
            if !only_tokens_after && !next_is_time {
                pieces[i] = Piece::Title(word.clone());
            }
        }
        only_tokens_after = only_tokens_after && !pieces[i].is_title();
    }

    let mut result = ParsedTask::default();
    let mut title: Vec<String> = vec![];
    let mut day: Option<NaiveDate> = None;
    let mut time: Option<(u32, u32)> = None;
    for piece in pieces {
        match piece {
            Piece::Title(w) => title.push(w),
            Piece::Group(g) => result.group_name = Some(g),
            Piece::Estimate(m) => result.estimate_min = Some(m),
            Piece::Priority(p) => result.priority = p,
            Piece::Day(d) | Piece::ShortDay(d, _) => day = Some(d),
            Piece::Time(h, m) => time = Some((h, m)),
        }
    }
    result.title = title.join(" ");
    if day.is_some() || time.is_some() {
        let base = day.unwrap_or(today);
        let (h, m) = time.unwrap_or((17, 0));
        let local = |d: NaiveDate| d.and_hms_opt(h, m, 0).and_then(|n| resolve_local(&Local, n));
        let mut due = local(base);
        // A bare time already past today means tomorrow.
        if day.is_none() {
            if let Some(d) = due {
                if d <= now {
                    due = local(base + Duration::days(1));
                }
            }
        }
        result.due = due;
    }
    result
}

/// A local wall-clock time as an instant. An ambiguous time (the clocks going back) is the
/// earlier one. A time the clocks skip (going forward) is the first valid time after the gap,
/// so "1:30am" on the night British clocks go from 1:00 to 2:00 is 2:00.
pub fn resolve_local<Tz: TimeZone>(tz: &Tz, naive: NaiveDateTime) -> Option<Date> {
    // Gaps are whole minutes and at most a day long.
    (0..=24 * 60).find_map(|m| match tz.from_local_datetime(&(naive + Duration::minutes(m))) {
        LocalResult::Single(t) => Some(t.with_timezone(&Utc)),
        // chrono's `earliest()` gives the first offset it lists, which is not always the earlier
        // instant (with TZ=Europe/London it is the later one).
        LocalResult::Ambiguous(a, b) => Some(a.with_timezone(&Utc).min(b.with_timezone(&Utc))),
        LocalResult::None => None,
    })
}

fn read(input: &str, groups: &[String], today: NaiveDate) -> Vec<Piece> {
    let words: Vec<&str> = input.split(' ').filter(|w| !w.is_empty()).collect();
    let mut pieces = vec![];
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        let lower = word.to_lowercase();
        i += 1;
        if (word.starts_with('@') || word.starts_with('#')) && word.chars().count() > 1 {
            let name = &word[1..];
            match groups.iter().find(|g| g.to_lowercase() == name.to_lowercase()) {
                Some(known) => pieces.push(Piece::Group(known.clone())),
                None => pieces.push(Piece::Title(word.to_string())),
            }
        } else if let Some(m) = lower.strip_prefix('~').and_then(minutes) {
            pieces.push(Piece::Estimate(m));
        } else if let Some(p) = lower.strip_prefix('!').and_then(priority) {
            pieces.push(Piece::Priority(p));
        } else if lower == "today" {
            pieces.push(Piece::Day(today));
        } else if lower == "tomorrow" || lower == "tmrw" {
            pieces.push(Piece::Day(today + Duration::days(1)));
        } else if lower == "next" && i < words.len() && words[i].to_lowercase() == "week" {
            pieces.push(Piece::Day(today + Duration::days(7)));
            i += 1;
        } else if let Some(wd) = WEEKDAYS.iter().position(|d| *d == lower) {
            pieces.push(Piece::Day(next_weekday(wd, today)));
        } else if lower.chars().count() == 3 && WEEKDAYS.iter().any(|d| d.starts_with(&lower)) {
            let wd = WEEKDAYS.iter().position(|d| d.starts_with(&lower)).unwrap_or(0);
            pieces.push(Piece::ShortDay(next_weekday(wd, today), word.to_string()));
        } else if let Some((h, m)) = clock(&lower) {
            pieces.push(Piece::Time(h, m));
        } else {
            pieces.push(Piece::Title(word.to_string()));
        }
    }
    pieces
}

/// The next such weekday after today; the same weekday means next week.
fn next_weekday(weekday: usize, today: NaiveDate) -> NaiveDate {
    let current = today.weekday().num_days_from_sunday() as i64;
    let mut delta = (weekday as i64 - current + 7) % 7;
    if delta == 0 {
        delta = 7;
    }
    today + Duration::days(delta)
}

/// "45m", "45min", "2h", "2hrs", "1h30m", "1h30", "90" -> minutes. None for anything that does
/// not fit an `i32`.
pub fn minutes(s: &str) -> Option<i32> {
    if let Ok(n) = s.parse::<i32>() {
        return if n > 0 { Some(n) } else { None };
    }
    // ^(?:(\d+)(?:h|hr|hrs|hour|hours))?(?:(\d+)(?:m|min|mins|minute|minutes)?)?$
    let mut rest = s;
    let mut total = 0i64;
    let digits = |r: &str| -> (String, usize) {
        let d: String = r.chars().take_while(|c| c.is_ascii_digit()).collect();
        let n = d.len();
        (d, n)
    };
    let (d, n) = digits(rest);
    if n > 0 {
        let after = &rest[n..];
        let hour_unit = ["hours", "hour", "hrs", "hr", "h"].iter().find(|u| after.starts_with(*u));
        if let Some(u) = hour_unit {
            total = total.checked_add(d.parse::<i64>().ok()?.checked_mul(60)?)?;
            rest = &after[u.len()..];
        }
    }
    let (d, n) = digits(rest);
    if n > 0 {
        let after = &rest[n..];
        let min_unit = ["minutes", "minute", "mins", "min", "m"].iter().find(|u| after == **u);
        if after.is_empty() || min_unit.is_some() {
            total = total.checked_add(d.parse::<i64>().ok()?)?;
            rest = "";
        } else {
            return None;
        }
    }
    if !rest.is_empty() {
        return None;
    }
    if total > 0 {
        i32::try_from(total).ok()
    } else {
        None
    }
}

pub fn priority(s: &str) -> Option<i32> {
    match s {
        "high" | "h" | "3" | "urgent" => Some(3),
        "med" | "medium" | "m" | "2" => Some(2),
        "low" | "l" | "1" => Some(1),
        _ => None,
    }
}

/// "3pm", "3:30pm", "15:00", "9am"
pub fn clock(s: &str) -> Option<(u32, u32)> {
    let mut body = s;
    let mut meridiem = None;
    if body.ends_with("am") || body.ends_with("pm") {
        meridiem = Some(&body[body.len() - 2..]);
        body = &body[..body.len() - 2];
    }
    let parts: Vec<&str> = body.split(':').collect();
    if parts.len() > 2 {
        return None;
    }
    let mut hour: i64 = parts[0].parse().ok()?;
    let mut minute = 0i64;
    if parts.len() == 2 {
        if parts[1].len() != 2 {
            return None;
        }
        minute = parts[1].parse().ok()?;
    } else if meridiem.is_none() {
        return None; // a bare number is not a time
    }
    if let Some(m) = meridiem {
        if !(1..=12).contains(&hour) {
            return None;
        }
        if m == "pm" && hour < 12 {
            hour += 12;
        }
        if m == "am" && hour == 12 {
            hour = 0;
        }
    }
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) {
        return None;
    }
    Some((hour as u32, minute as u32))
}

/// The task to suggest next: unfinished tasks in the group, earliest due first, then by
/// priority. A task whose estimate fits `available_min` beats one that does not.
pub fn suggest(tasks: &[TaskItem], group_id: Option<Id>, excluding: Option<Id>, available_min: Option<i32>) -> Option<TaskItem> {
    let fits = |t: &TaskItem| match (available_min, t.estimate_min) {
        (Some(a), Some(e)) => e <= a,
        _ => true,
    };
    let mut pool: Vec<&TaskItem> = tasks
        .iter()
        .filter(|t| {
            t.status == TaskStatus::Todo && Some(t.id) != excluding && t.parent_id.is_none() && (group_id.is_none() || t.group_id == group_id)
        })
        .collect();
    pool.sort_by(|a, b| {
        use std::cmp::Ordering;
        if fits(a) != fits(b) {
            return if fits(a) { Ordering::Less } else { Ordering::Greater };
        }
        match (a.due, b.due) {
            (Some(x), Some(y)) if x != y => return x.cmp(&y),
            (Some(_), None) => return Ordering::Less,
            (None, Some(_)) => return Ordering::Greater,
            _ => {}
        }
        if a.priority != b.priority {
            return b.priority.cmp(&a.priority);
        }
        a.created_at.cmp(&b.created_at)
    });
    pool.first().map(|t| (*t).clone())
}
