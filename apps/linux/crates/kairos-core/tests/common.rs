// Shared helpers, included with `mod common;` by each test file.
#![allow(dead_code)]

use chrono::{Local, TimeZone, Utc};
use kairos_core::Date;
use std::path::PathBuf;

/// A UTC instant from Unix seconds (may be fractional).
pub fn t(secs: f64) -> Date {
    Utc.timestamp_millis_opt((secs * 1000.0).round() as i64).single().unwrap()
}

pub fn plus(d: Date, secs: f64) -> Date {
    kairos_core::time::add(d, secs)
}

/// A local wall-clock time, as the Rust core reads dates in the local zone.
pub fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Date {
    Local.with_ymd_and_hms(y, mo, d, h, mi, 0).earliest().unwrap().with_timezone(&Utc)
}

/// A fresh, empty folder under the system temp directory.
pub fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kairos-{tag}-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Points KAIROS_HOME at a scratch folder for this test binary, so nothing touches a real profile.
pub fn scratch_home() -> PathBuf {
    static HOME: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        let dir = temp_dir("home");
        std::env::set_var("KAIROS_HOME", &dir);
        dir
    })
    .clone()
}

pub fn approx(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}
