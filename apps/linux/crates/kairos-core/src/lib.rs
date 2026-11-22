//! Kairos's logic, shared by the Linux app and its native messaging host. A port of the macOS
//! app's KairosCore: everything here is free of GTK and X11, so it is testable on its own.

pub mod appguard;
pub mod bridge;
pub mod browsers;
pub mod claude;
pub mod clearance;
pub mod dock;
pub mod duetext;
pub mod history;
pub mod hotkeys;
pub mod id;
pub mod import;
pub mod jev;
pub mod keys;
pub mod lenient;
pub mod models;
pub mod node;
pub mod panel;
pub mod privacy;
pub mod quickadd;
pub mod reminders;
pub mod rules;
pub mod scenes;
pub mod setup;
pub mod store;
pub mod tempo;
pub mod time;
pub mod visibility;
pub mod voice;

pub use id::Id;
pub use models::*;
pub use time::Date;
