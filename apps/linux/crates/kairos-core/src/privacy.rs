//! `Settings.never_analyse` covers every path that sends something from a page off the machine:
//! Jev, cloud voices and scenes (PROTOCOL.md, "Privacy").

use crate::bridge::Message;
use crate::browsers;
use crate::jev::Subject;
use crate::rules::{host_in, host_of};

/// Bridge requests whose text goes to a cloud service. Kairos Read passes the page's `url`.
pub const CONTENT_REQUESTS: &[&str] = &["tts.speak", "tts.prefetch", "scene.candidates", "scene.generate", "scene.edit"];

/// The error to answer a request with when it carries text from a listed page, or None.
pub fn refusal(message: &Message, never_analyse: &[String]) -> Option<String> {
    let kind = message.get("type")?.as_str()?;
    if !CONTENT_REQUESTS.contains(&kind) {
        return None;
    }
    let url = message.get("url")?.as_str()?;
    if url.is_empty() {
        return None;
    }
    let host = host_of(url);
    if host.is_empty() || !host_in(&host, never_analyse) {
        return None;
    }
    Some(format!(
        "{host} is on your \u{201C}Never send off this computer\u{201D} list in Kairos, so nothing from it goes to a cloud service."
    ))
}

/// What Jev is told about an app. A browser's window title is the title of its page, so a
/// browser is sent by name only.
pub fn app_subject(name: &str, app: &str, window_title: &str) -> Subject {
    let mut s = Subject::new("app");
    s.app = name.to_string();
    s.title = if browsers::is_browser(app) { String::new() } else { window_title.to_string() };
    s
}
