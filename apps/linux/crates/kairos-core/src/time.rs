//! Dates are UTC instants. They are written as ISO 8601 with whole seconds, the form Swift's
//! `.iso8601` strategy reads and writes, so exports move between the Mac and Linux apps.

use chrono::{DateTime, Local, SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Deserializer, Serializer};

pub type Date = DateTime<Utc>;

pub fn now() -> Date {
    Utc::now()
}

/// Seconds from `b` to `a`, as `a.timeIntervalSince(b)`.
pub fn since(a: Date, b: Date) -> f64 {
    (a - b).num_milliseconds() as f64 / 1000.0
}

pub fn add(d: Date, seconds: f64) -> Date {
    d + chrono::Duration::milliseconds((seconds * 1000.0).round() as i64)
}

pub fn distant_past() -> Date {
    Utc.timestamp_opt(-62135769600, 0).single().unwrap_or_default()
}

pub fn ms(d: Date) -> i64 {
    d.timestamp_millis()
}

pub fn local(d: Date) -> DateTime<Local> {
    d.with_timezone(&Local)
}

/// "15:00" in the locale's 24-hour form (Mint defaults to it in most locales).
pub fn short_time(d: Date) -> String {
    local(d).format("%H:%M").to_string()
}

pub fn format(d: &Date) -> String {
    d.to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub fn parse(s: &str) -> Option<Date> {
    DateTime::parse_from_rfc3339(s.trim()).ok().map(|d| d.with_timezone(&Utc))
}

pub mod iso {
    use super::*;
    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format(d))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Date, D::Error> {
        let s = String::deserialize(d)?;
        parse(&s).ok_or_else(|| serde::de::Error::custom("not an ISO 8601 date"))
    }
}

pub mod iso_opt {
    use super::*;
    pub fn serialize<S: Serializer>(d: &Option<Date>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_str(&format(d)),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Date>, D::Error> {
        let s: Option<String> = Option::deserialize(d)?;
        match s {
            None => Ok(None),
            Some(s) => parse(&s).map(Some).ok_or_else(|| serde::de::Error::custom("not an ISO 8601 date")),
        }
    }
}
