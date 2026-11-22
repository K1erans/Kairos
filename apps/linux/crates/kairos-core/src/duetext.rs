//! A due date as short as a task row can show it: "Today 15:00", "Tomorrow 09:00",
//! "Yesterday 15:00", "Wed 15:00" within the coming week, "20 Nov 15:00" this year, and
//! "5 Jan 2027" further off.

use crate::time::Date;
use chrono::{DateTime, Datelike, TimeZone};

pub fn short<Tz: TimeZone>(due: Date, now: Date, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let d: DateTime<Tz> = due.with_timezone(tz);
    let n: DateTime<Tz> = now.with_timezone(tz);
    let time = d.format("%H:%M").to_string();
    let days = (d.date_naive() - n.date_naive()).num_days();
    match days {
        0 => format!("Today {time}"),
        1 => format!("Tomorrow {time}"),
        -1 => format!("Yesterday {time}"),
        2..=6 => format!("{} {time}", d.format("%a")),
        _ => {
            if d.year() == n.year() {
                format!("{} {time}", d.format("%-d %b"))
            } else {
                d.format("%-d %b %Y").to_string()
            }
        }
    }
}

pub fn short_local(due: Date, now: Date) -> String {
    short(due, now, &chrono::Local)
}
