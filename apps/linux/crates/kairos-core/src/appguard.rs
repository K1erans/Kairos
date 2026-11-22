//! Decides which apps the app guard hides. Pure, so the rules are testable without running apps;
//! the app feeds it what the window manager reports.

use crate::browsers;
use crate::models::{AppGroup, Mode};

/// Hiding these would lock the owner out of their desktop or out of Kairos itself.
pub const NEVER_GUARD: &[&str] = &[
    "nemo.desktop",
    "cinnamon-settings.desktop",
    "org.gnome.Terminal.desktop",
    "org.gnome.SystemMonitor.desktop",
    "gnome-system-monitor.desktop",
    "gnome-system-monitor-kde.desktop",
    "xfce4-terminal.desktop",
    "mate-terminal.desktop",
    "Alacritty.desktop",
    "kitty.desktop",
    "org.wezfurlong.wezterm.desktop",
    "cinnamon-settings-users.desktop",
    "mintupdate.desktop",
    "kairos.desktop",
];

pub fn never_guard(app: &str) -> bool {
    NEVER_GUARD.contains(&app) || app.starts_with("cinnamon-settings")
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuardApp {
    pub app: String,
    /// On screen: not minimised by the owner or by Kairos.
    pub visible: bool,
}

/// A Chromium browser that has just opened needs a moment before its Shield connects.
pub const SHIELD_GRACE: f64 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// On the group's blocked apps.
    Listed,
    /// A browser Kairos Shield is not running in, where site rules cannot reach.
    BrowserWithoutShield,
}

pub struct Context<'a> {
    pub mode: Mode,
    pub group: Option<&'a AppGroup>,
    pub own: Option<&'a str>,
    pub shield_connected: bool,
    pub blocks_unshielded_browsers: bool,
    pub is_allowed: &'a dyn Fn(&str) -> bool,
}

impl<'a> Context<'a> {
    /// Why this app is blocked right now, or None. Jev is asked separately.
    pub fn reason(&self, app: &str) -> Option<Reason> {
        let group = self.group?;
        if !self.mode.blocks() || Some(app) == self.own || never_guard(app) || (self.is_allowed)(app) {
            return None;
        }
        if group.blocked_apps.iter().any(|a| a == app) {
            return Some(Reason::Listed);
        }
        if self.blocks_unshielded_browsers
            && browsers::is_browser(app)
            && !(browsers::is_chromium(app) && self.shield_connected)
        {
            return Some(Reason::BrowserWithoutShield);
        }
        None
    }

    /// The visible apps to hide when blocking starts, the group changes or an "Allow" runs out.
    pub fn sweep(&self, apps: &[GuardApp]) -> Vec<(String, Reason)> {
        apps.iter().filter(|a| a.visible).filter_map(|a| self.reason(&a.app).map(|r| (a.app.clone(), r))).collect()
    }
}
