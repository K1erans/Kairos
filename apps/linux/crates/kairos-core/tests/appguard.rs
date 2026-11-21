use kairos_core::appguard::{Context, GuardApp, Reason};
use kairos_core::browsers;
use kairos_core::{AppData, AppGroup, Mode};
use std::collections::HashSet;

fn group() -> AppGroup {
    let mut g = AppGroup::new("Work");
    g.blocked_apps = vec!["discord.desktop".into(), "spotify.desktop".into()];
    g.dock_apps = vec!["xed.desktop".into()];
    g
}
fn ga(app: &str, visible: bool) -> GuardApp {
    GuardApp { app: app.into(), visible }
}

fn sweep(apps: &[GuardApp], mode: Mode, allowed: &[&str], group: Option<AppGroup>) -> Vec<String> {
    let g = group.unwrap_or_else(self::group);
    let allowed: HashSet<String> = allowed.iter().map(|s| s.to_string()).collect();
    let is_allowed = |a: &str| allowed.contains(a);
    Context { mode, group: Some(&g), own: Some("kairos.desktop"), shield_connected: true, blocks_unshielded_browsers: true, is_allowed: &is_allowed }
        .sweep(apps)
        .into_iter()
        .map(|(a, _)| a)
        .collect()
}

#[test]
fn a_sweep_hides_blocked_apps_that_are_already_on_screen() {
    let apps = [ga("discord.desktop", true), ga("xed.desktop", true), ga("spotify.desktop", true)];
    assert_eq!(sweep(&apps, Mode::Focus, &[], None), vec!["discord.desktop", "spotify.desktop"]);
    assert_eq!(sweep(&apps, Mode::Strict, &[], None), vec!["discord.desktop", "spotify.desktop"]);
}

#[test]
fn it_leaves_hidden_allowed_and_unblocked_apps_alone() {
    let apps = [ga("discord.desktop", false), ga("spotify.desktop", true)];
    assert_eq!(sweep(&apps, Mode::Focus, &[], None), vec!["spotify.desktop"], "an app already hidden is not hidden again");
    assert!(sweep(&apps, Mode::Focus, &["spotify.desktop"], None).is_empty(), "an unexpired Allow wins");
    assert!(sweep(&apps, Mode::Off, &[], None).is_empty());
    assert!(sweep(&apps, Mode::OnBreak, &[], None).is_empty());
}

#[test]
fn kairos_and_the_system_are_never_hidden() {
    let mut g = group();
    g.blocked_apps.extend(["nemo.desktop".to_string(), "kairos.desktop".to_string(), "cinnamon-settings-display.desktop".to_string()]);
    let apps = [ga("nemo.desktop", true), ga("kairos.desktop", true), ga("cinnamon-settings-display.desktop", true)];
    assert!(sweep(&apps, Mode::Focus, &[], Some(g)).is_empty());
}

// MARK: UnshieldedBrowserTests

fn reason(app: &str, shield: bool, setting: bool, mode: Mode, allowed: bool, group: Option<AppGroup>) -> Option<Reason> {
    let g = group.unwrap_or_else(|| AppGroup::new("Work"));
    let is_allowed = move |_: &str| allowed;
    Context { mode, group: Some(&g), own: Some("kairos.desktop"), shield_connected: shield, blocks_unshielded_browsers: setting, is_allowed: &is_allowed }
        .reason(app)
}
fn r(app: &str, shield: bool) -> Option<Reason> {
    reason(app, shield, true, Mode::Focus, false, None)
}

#[test]
fn browsers_shield_cannot_run_in_are_blocked_while_blocking() {
    for app in ["firefox.desktop", "org.gnome.Epiphany.desktop"] {
        assert_eq!(r(app, true), Some(Reason::BrowserWithoutShield), "{app}");
        assert_eq!(r(app, false), Some(Reason::BrowserWithoutShield), "{app}");
    }
}

#[test]
fn a_chromium_browser_is_blocked_only_while_no_shield_is_connected() {
    for app in ["google-chrome.desktop", "brave-browser.desktop", "chromium.desktop"] {
        assert_eq!(r(app, false), Some(Reason::BrowserWithoutShield), "{app}");
        assert_eq!(r(app, true), None, "any connected Shield is taken to cover every Chromium browser");
    }
}

#[test]
fn the_setting_mode_and_an_allow_turn_it_off() {
    assert_eq!(reason("firefox.desktop", false, false, Mode::Focus, false, None), None);
    assert_eq!(reason("firefox.desktop", false, true, Mode::Off, false, None), None);
    assert_eq!(reason("firefox.desktop", false, true, Mode::OnBreak, false, None), None);
    assert_eq!(reason("firefox.desktop", false, true, Mode::Focus, true, None), None);
    assert_eq!(r("xed.desktop", false), None, "not a browser");
}

#[test]
fn a_listed_block_says_so_first() {
    let mut g = AppGroup::new("Work");
    g.blocked_apps = vec!["firefox.desktop".into()];
    assert_eq!(reason("firefox.desktop", false, true, Mode::Focus, false, Some(g)), Some(Reason::Listed));
}

#[test]
fn the_starter_pins_an_installed_chromium_browser() {
    let brave = AppData::starter(browsers::preferred_chromium(&|id| id == "brave-browser.desktop"), &|_| true);
    assert_eq!(brave.groups[0].dock_apps[0], "brave-browser.desktop");
    assert!(brave.groups[1].dock_apps.contains(&"brave-browser.desktop".to_string()));
    let none = AppData::starter(browsers::preferred_chromium(&|_| false), &|_| false);
    assert!(none.groups.iter().all(|g| g.dock_apps.iter().all(|a| !browsers::is_browser(a))), "no Chromium browser: none pinned");
    assert!(none.groups.iter().all(|g| g.dock_apps.is_empty()), "apps that are not installed stay off the bar");
}
