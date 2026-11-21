mod common;
use common::*;
use kairos_core::rules::{bridge_state, host_matches, host_of, normalize_site, rule_lock, OverrideLedger, Refusal, SiteList};
use kairos_core::{AppData, AppGroup, Mode};
use serde_json::Value;
use std::collections::BTreeMap;

fn starter() -> AppData {
    AppData::starter(None, &|_| true)
}

// MARK: RulesTests

#[test]
fn host_matching() {
    assert!(host_matches("www.youtube.com", "youtube.com"));
    assert!(host_matches("m.youtube.com", "https://YouTube.com/feed"));
    assert!(!host_matches("notyoutube.com", "youtube.com"));
    assert!(!host_matches("youtube.com", ""));
    assert_eq!(host_of("https://www.reddit.com/r/swift?x=1"), "reddit.com");
    assert_eq!(normalize_site("https://www.Docs.Google.com:443/document/1"), "docs.google.com");
}

#[test]
fn override_ledger() {
    let mut l = OverrideLedger::new();
    let now = t(0.0);
    assert_eq!(l.request("x.com", "", Mode::Focus, 3, 5, now), Err(Refusal::NoReason));
    assert_eq!(l.request("x.com", "need a link", Mode::Strict, 3, 5, now), Err(Refusal::Strict));
    assert_eq!(l.request("x.com", "need a link", Mode::Focus, 1, 5, now), Ok(t(300.0)));
    assert!(l.is_allowed("mobile.x.com", t(299.0)));
    assert!(!l.is_allowed("x.com", t(301.0)));
    assert_eq!(l.request("y.com", "again", Mode::Focus, 1, 5, now), Err(Refusal::Exhausted));
    l.reset();
    assert_eq!(l.remaining(1), 1);
}

#[test]
fn the_next_grant_to_run_out_is_the_earliest_unexpired_one() {
    let now = t(1_000_000.0);
    let mut ledger = OverrideLedger::new();
    assert_eq!(ledger.next_expiry(now), None);
    let _ = ledger.request("a.com", "r", Mode::Focus, 3, 5, now);
    let _ = ledger.request("b.com", "r", Mode::Focus, 3, 5, plus(now, 60.0));
    assert_eq!(ledger.next_expiry(now), Some(plus(now, 300.0)));
    assert_eq!(ledger.next_expiry(plus(now, 301.0)), Some(plus(now, 360.0)));
    assert_eq!(ledger.next_expiry(plus(now, 400.0)), None);
}

#[test]
fn state_payload_is_valid_json_with_nulls() {
    let mut data = starter();
    data.focus.mode = Mode::Focus;
    let payload = bridge_state(&data, &OverrideLedger::new(), false, t(0.0));
    assert!(payload.is_object());
    assert_eq!(payload["currentTask"], Value::Null);
    assert_eq!(payload["mode"], "focus");
    assert_eq!(payload["overridesLeft"], 3);
    assert_eq!(payload["rules"]["jevFailClosed"], false, "Shield follows it when Jev is slow");
    data.settings.jev_fail_closed = true;
    assert_eq!(bridge_state(&data, &OverrideLedger::new(), false, t(0.0))["rules"]["jevFailClosed"], true);
    let blocks = payload["rules"]["blockSites"].as_array().unwrap();
    assert!(blocks.iter().any(|s| s == "youtube.com"));
}

#[test]
fn the_state_pushed_to_chrome_carries_strict_until_and_the_override_length() {
    let mut d = starter();
    d.settings.override_minutes = 7;
    let payload = bridge_state(&d, &OverrideLedger::new(), false, t(0.0));
    assert_eq!(payload["overrideMinutes"], 7);
    assert_eq!(payload["strictUntil"], Value::Null);
    d.focus.mode = Mode::Strict;
    d.focus.strict_until = Some(t(1_794_252_486.430));
    let payload = bridge_state(&d, &OverrideLedger::new(), false, t(0.0));
    assert_eq!(payload["strictUntil"].as_i64(), Some(1_794_252_486_430));
    assert_eq!(payload["overridesLeft"], 0, "Strict has no overrides");
}

// MARK: RuleLockTests

fn work() -> AppGroup {
    let mut g = AppGroup::new("Work");
    g.dock_apps = vec!["firefox.desktop".into(), "code.desktop".into(), "nemo.desktop".into()];
    g.block_sites = vec!["youtube.com".into()];
    g.allow_sites = vec!["docs.google.com".into()];
    g.blocked_apps = vec!["discord.desktop".into()];
    g.hidden_elements = BTreeMap::from([("news.com".to_string(), vec!["#promo".to_string()])]);
    g.jev_enabled = true;
    g.jev_threshold = 0.7;
    g
}

fn data() -> AppData {
    let mut d = AppData::default();
    d.groups = vec![work(), AppGroup::new("Study")];
    d.focus.active_group_id = Some(d.groups[0].id);
    d
}

#[test]
fn what_loosens_the_rules() {
    let old = work();
    let looser: Vec<(&str, Box<dyn Fn(&mut AppGroup)>)> = vec![
        ("unblocking a site", Box::new(|g| g.block_sites.clear())),
        ("allowing a site", Box::new(|g| g.allow_sites.push("reddit.com".into()))),
        ("unblocking an app", Box::new(|g| g.blocked_apps.clear())),
        ("showing a hidden element", Box::new(|g| g.show_element("#promo", "news.com"))),
        ("turning Jev off", Box::new(|g| g.jev_enabled = false)),
        ("making Jev slower to block", Box::new(|g| g.jev_threshold = 0.9)),
        ("swapping a blocked site for another", Box::new(|g| g.block_sites = vec!["reddit.com".into()])),
    ];
    for (what, change) in looser {
        let mut new = old.clone();
        change(&mut new);
        assert!(rule_lock::loosens(&old, &new), "{what}");
    }
}

#[test]
fn what_only_tightens_or_leaves_the_rules_alone() {
    let old = work();
    let fine: Vec<(&str, Box<dyn Fn(&mut AppGroup)>)> = vec![
        ("blocking a site", Box::new(|g| g.block_sites.push("reddit.com".into()))),
        ("taking a site off the allowed list", Box::new(|g| g.allow_sites.clear())),
        ("blocking an app", Box::new(|g| g.blocked_apps.push("steam.desktop".into()))),
        ("hiding another element", Box::new(|g| g.hide_element("#related", "youtube.com"))),
        ("making Jev quicker to block", Box::new(|g| g.jev_threshold = 0.6)),
        (
            "renaming and recolouring",
            Box::new(|g| {
                g.name = "Deep work".into();
                g.color_hex = "#FF0000".into();
            }),
        ),
        ("unpinning apps from Kairos Bar", Box::new(|g| g.dock_apps = vec!["code.desktop".into()])),
        ("reordering the apps on Kairos Bar", Box::new(|g| g.dock_apps = vec!["code.desktop".into(), "nemo.desktop".into(), "firefox.desktop".into()])),
    ];
    for (what, change) in fine {
        let mut new = old.clone();
        change(&mut new);
        assert!(!rule_lock::loosens(&old, &new), "{what}");
    }
    let mut off = old.clone();
    off.jev_enabled = false;
    let mut on = off.clone();
    on.jev_enabled = true;
    on.jev_threshold = 0.95;
    assert!(!rule_lock::loosens(&off, &on), "turning Jev on is stricter at any threshold");
}

#[test]
fn the_rules_page_cannot_loosen_the_active_group() {
    let mut d = data();
    let id = d.groups[0].id;
    let before = d.groups[0].clone();
    assert!(!d.update_group(id, true, |g| g.block_sites.clear()));
    assert!(!d.update_group(id, true, |g| g.allow_sites.push("reddit.com".into())));
    assert!(!d.update_group(id, true, |g| g.jev_enabled = false));
    assert!(
        !d.update_group(id, true, |g| {
            g.block_sites.push("reddit.com".into());
            g.blocked_apps.clear();
        }),
        "a change that loosens anything is refused as a whole"
    );
    assert_eq!(d.groups[0], before, "nothing changed");

    assert!(d.update_group(id, true, |g| g.block_sites.push("reddit.com".into())), "adding a block is fine");
    assert!(d.update_group(id, true, |g| g.name = "Deep work".into()));
    assert_eq!(d.groups[0].block_sites, vec!["youtube.com", "reddit.com"]);
    assert!(d.update_group(id, false, |g| g.block_sites.clear()), "once the phase ends it is free again");
}

#[test]
fn other_groups_stay_editable() {
    let mut d = data();
    let study = d.groups[1].id;
    assert!(d.update_group(study, true, |g| g.allow_sites = vec!["reddit.com".into()]), "only the active group's rules are in force");
}

#[test]
fn the_bridge_cannot_show_a_hidden_element_or_allow_a_site() {
    let mut d = data();
    assert!(!d.set_element_hidden("#promo", "news.com", false, true), "rule.removeElement");
    assert_eq!(d.active_group().unwrap().hidden_elements.get("news.com"), Some(&vec!["#promo".to_string()]));
    assert!(d.set_element_hidden("#related", "youtube.com", true, true), "rule.addElement");
    assert!(!d.add_site("reddit.com", SiteList::Allow, true), "rule.addSite allow");
    assert!(d.add_site("docs.google.com", SiteList::Block, true), "rule.addSite block, even off the allowed list");
    assert!(d.active_group().unwrap().allow_sites.is_empty());
    assert!(!d.set_app_blocked("discord.desktop", false, true), "Kairos Bar's Unblock");
}

#[test]
fn the_line_says_when_it_unlocks() {
    let at = local(2026, 11, 9, 10, 45);
    let note = rule_lock::note(Some(at));
    assert!(note.starts_with(&format!("Strict focus until {}:", kairos_core::time::short_time(at))), "{note}");
    assert!(note.contains("10:45"), "{note}");
    assert!(note.contains("block more"));
    assert!(rule_lock::note(None).contains("until this session ends"));
}

// MARK: AddSiteTests

fn site_data() -> AppData {
    let mut d = AppData::default();
    let mut g = AppGroup::new("Work");
    g.block_sites = vec!["youtube.com".into()];
    g.allow_sites = vec!["docs.google.com".into()];
    d.focus.active_group_id = Some(g.id);
    d.groups = vec![g, AppGroup::new("Study")];
    d
}

#[test]
fn blocking_a_site_moves_it_off_the_allow_list() {
    let mut d = site_data();
    assert!(d.add_site("https://www.Docs.Google.com/document/1", SiteList::Block, false));
    assert_eq!(d.active_group().unwrap().block_sites, vec!["youtube.com", "docs.google.com"]);
    assert!(d.active_group().unwrap().allow_sites.is_empty());
    assert!(d.groups[1].block_sites.is_empty(), "only the active group changes");
}

#[test]
fn always_allow_moves_it_off_the_block_list() {
    let mut d = site_data();
    assert!(d.add_site("youtube.com", SiteList::Allow, false));
    assert!(d.active_group().unwrap().block_sites.is_empty());
    assert_eq!(d.active_group().unwrap().allow_sites, vec!["docs.google.com", "youtube.com"]);
}

#[test]
fn adding_what_is_already_there_changes_nothing() {
    let mut d = site_data();
    assert!(!d.add_site("youtube.com", SiteList::Block, false));
    assert!(!d.add_site("", SiteList::Block, false));
    assert_eq!(SiteList::parse("block"), Some(SiteList::Block));
    assert_eq!(SiteList::parse("nope"), None);
}

#[test]
fn strict_refuses_an_allow_but_still_takes_a_block() {
    let mut d = site_data();
    assert!(!d.add_site("youtube.com", SiteList::Allow, true));
    assert_eq!(d.active_group().unwrap().block_sites, vec!["youtube.com"]);
    assert!(d.add_site("reddit.com", SiteList::Block, true));
}

#[test]
fn blocking_an_app_from_the_bar_and_unblocking_it_outside_strict() {
    let mut d = site_data();
    assert!(d.set_app_blocked("discord.desktop", true, true), "blocking more is fine in Strict");
    assert_eq!(d.active_group().unwrap().blocked_apps, vec!["discord.desktop"]);
    assert!(!d.set_app_blocked("discord.desktop", false, true));
    assert!(d.set_app_blocked("discord.desktop", false, false));
    assert!(d.active_group().unwrap().blocked_apps.is_empty());
}

#[test]
fn hidden_elements_add_and_remove() {
    let mut d = starter();
    let mut hide = |d: &mut AppData, sel: &str, host: &str, on: bool| d.set_element_hidden(sel, host, on, false);
    assert!(hide(&mut d, "#a", "https://www.News.com/x", true));
    assert!(!hide(&mut d, "#a", "news.com", true), "already hidden");
    assert!(hide(&mut d, "#b", "news.com", true));
    assert_eq!(d.active_group().unwrap().hidden_elements["news.com"], vec!["#a", "#b"]);
    assert!(hide(&mut d, "#a", "news.com", false));
    assert_eq!(d.active_group().unwrap().hidden_elements["news.com"], vec!["#b"]);
    assert!(hide(&mut d, "#b", "news.com", false));
    assert!(d.active_group().unwrap().hidden_elements.get("news.com").is_none(), "no empty entry is left behind");
    assert!(!hide(&mut d, "#zz", "other.com", false));
    assert!(!hide(&mut d, "", "news.com", true));
    assert!(d.active_group().unwrap().hidden_elements.is_empty());
}

// MARK: Regression tests for the 2026-11-21 review

#[test]
fn pinning_an_app_to_kairos_bar_loosens_while_jev_is_on() {
    // Jev skips apps on the group's Kairos Bar, so pinning one during Strict lets it through.
    let old = work();
    let mut new = old.clone();
    new.dock_apps.push("discord.desktop".into());
    assert!(rule_lock::loosens(&old, &new));
    let mut d = data();
    let id = d.groups[0].id;
    assert!(!d.update_group(id, true, |g| g.dock_apps.push("steam.desktop".into())), "refused during a strict focus phase");
    assert!(d.update_group(id, true, |g| g.dock_apps.retain(|a| a != "code.desktop")), "unpinning is fine");
    assert!(d.update_group(id, true, |g| g.dock_apps.reverse()), "reordering is fine");
    // Without Jev nothing judges apps by the pinned list, so pinning blocks nothing less.
    let mut off = old.clone();
    off.jev_enabled = false;
    let mut pinned = off.clone();
    pinned.dock_apps.push("discord.desktop".into());
    assert!(!rule_lock::loosens(&off, &pinned));
}

#[test]
fn settings_loosen_only_when_blocking_gets_weaker() {
    let old = kairos_core::Settings { block_unshielded_browsers: true, jev_fail_closed: true, ..Default::default() };
    let looser: Vec<(&str, Box<dyn Fn(&mut kairos_core::Settings)>)> = vec![
        ("letting browsers without Shield through", Box::new(|s| s.block_unshielded_browsers = false)),
        ("letting pages Jev can't judge through", Box::new(|s| s.jev_fail_closed = false)),
        ("taking a site out of Jev's checks", Box::new(|s| s.never_analyse.push("reddit.com".into()))),
    ];
    for (what, change) in looser {
        let mut new = old.clone();
        change(&mut new);
        assert!(rule_lock::settings_loosen(&old, &new), "{what}");
    }
    let fine: Vec<(&str, Box<dyn Fn(&mut kairos_core::Settings)>)> = vec![
        ("giving a site back to Jev", Box::new(|s| s.never_analyse.clear())),
        ("changing the override length", Box::new(|s| s.override_minutes = 10)),
    ];
    for (what, change) in fine {
        let mut new = old.clone();
        change(&mut new);
        assert!(!rule_lock::settings_loosen(&old, &new), "{what}");
    }
    let mut off = kairos_core::Settings { block_unshielded_browsers: false, jev_fail_closed: false, ..Default::default() };
    let before = off.clone();
    off.block_unshielded_browsers = true;
    off.jev_fail_closed = true;
    assert!(!rule_lock::settings_loosen(&before, &off), "turning either on is stricter");
}

#[test]
fn update_settings_refuses_a_loosening_change_as_a_whole_while_locked() {
    let mut d = data();
    d.settings.block_unshielded_browsers = true;
    let before = d.settings.clone();
    assert!(!d.update_settings(true, |s| s.block_unshielded_browsers = false));
    assert!(
        !d.update_settings(true, |s| {
            s.override_minutes = 10;
            s.never_analyse.push("reddit.com".into());
        }),
        "refused as a whole"
    );
    assert_eq!(d.settings, before);
    assert!(!d.update_settings(true, |_| {}), "no change");
    assert!(d.update_settings(true, |s| s.override_minutes = 10), "a change that loosens nothing goes through");
    assert_eq!(d.settings.override_minutes, 10);
    assert!(d.update_settings(false, |s| s.block_unshielded_browsers = false), "free again once unlocked");
    assert!(!d.settings.block_unshielded_browsers);
}

#[test]
fn an_app_override_covers_only_that_app() {
    let mut l = OverrideLedger::new();
    let now = t(0.0);
    assert!(l.request("discord.desktop", "call", Mode::Focus, 3, 5, now).is_ok());
    assert!(l.is_allowed("discord.desktop", t(10.0)));
    assert!(!l.is_allowed("evil.discord.desktop", t(10.0)));
    assert!(!l.is_allowed("steam.desktop", t(10.0)));
}

#[test]
fn an_override_for_a_bare_suffix_is_refused_and_never_matches() {
    let mut l = OverrideLedger::new();
    let now = t(0.0);
    for target in ["desktop", "com", "localhost", "co.uk", ".com", ""] {
        assert_eq!(l.request(target, "why", Mode::Focus, 9, 5, now), Err(Refusal::NotBlocked), "{target:?}");
    }
    assert_eq!(l.used(), 0, "a refused request uses no override");
    assert!(!l.is_allowed("steam.desktop", t(10.0)));
    assert!(!l.is_allowed("youtube.com", t(10.0)));
    assert!(l.request("bbc.co.uk", "news", Mode::Focus, 9, 5, now).is_ok());
    assert!(l.is_allowed("www.bbc.co.uk", t(10.0)));
    assert!(!l.is_allowed("itv.co.uk", t(10.0)));
}

#[test]
fn what_counts_as_one_target() {
    use kairos_core::rules::{is_app_target, is_site_target, target_matches};
    assert!(is_app_target("discord.desktop"));
    assert!(!is_app_target("discord.com"));
    for good in ["youtube.com", "m.youtube.com", "bbc.co.uk", "127.0.0.1", "::1", "[::1]"] {
        assert!(is_site_target(good), "{good}");
    }
    for bad in ["com", "desktop", "localhost", "co.uk", "github.io", "a..com", "", "discord.desktop"] {
        assert!(!is_site_target(bad), "{bad}");
    }
    assert!(target_matches("m.youtube.com", "youtube.com"));
    assert!(!target_matches("youtube.com", "com"));
    assert!(!target_matches("x.desktop", "desktop"));
    assert!(target_matches("com", "com"), "an exact record still matches itself");
}

#[test]
fn only_targets_blocked_right_now_can_be_overridden() {
    let mut d = data();
    d.groups[0].jev_enabled = false;
    d.focus.mode = Mode::Off;
    assert!(!d.is_blocked_target("youtube.com", true), "nothing blocks while off");
    d.focus.mode = Mode::Focus;
    assert!(d.is_blocked_target("youtube.com", true));
    assert!(d.is_blocked_target("m.youtube.com", true));
    assert!(!d.is_blocked_target("docs.google.com", true), "on no block list");
    assert!(!d.is_blocked_target("com", true));
    assert!(d.is_blocked_target("discord.desktop", true), "a blocked app");
    assert!(!d.is_blocked_target("code.desktop", true));
    assert!(!d.is_blocked_target("desktop", true));
    assert!(!d.is_blocked_target("kairos.desktop", true));
    d.settings.block_unshielded_browsers = true;
    assert!(d.is_blocked_target("firefox.desktop", true), "a browser Shield can't run in");
    assert!(!d.is_blocked_target("google-chrome.desktop", true), "Shield is connected");
    assert!(d.is_blocked_target("google-chrome.desktop", false), "Shield is not connected");
    // With Jev on, anything Jev may judge can be blocked.
    d.groups[0].jev_enabled = true;
    assert!(d.is_blocked_target("reddit.com", true));
    assert!(d.is_blocked_target("steam.desktop", true));
    assert!(!d.is_blocked_target("code.desktop", true), "Jev skips apps on Kairos Bar");
    assert!(!d.is_blocked_target("docs.google.com", true), "always allowed");
}

#[test]
fn normalize_site_handles_trailing_dots_wildcards_and_ipv6() {
    assert_eq!(normalize_site("youtube.com."), "youtube.com");
    assert!(host_matches("youtube.com.", "youtube.com"));
    assert!(host_matches("m.youtube.com", "youtube.com."));
    assert_eq!(normalize_site("*.youtube.com"), "youtube.com");
    assert_eq!(normalize_site("http://[::1]:8080/"), "::1");
    assert_eq!(normalize_site("[2001:db8::1]"), "2001:db8::1");
    assert_eq!(normalize_site("2001:db8::1"), "2001:db8::1");
    assert_eq!(normalize_site("localhost:3000"), "localhost");
    assert_eq!(normalize_site("https://user:pw@www.Example.com:8443/x?y#z"), "example.com");
    assert_eq!(normalize_site("example.com?q=1"), "example.com");
    assert_eq!(normalize_site("MÜNCHEN.de"), "münchen.de", "lower-cased; not converted to punycode");
    assert_eq!(host_of("https://mail.google.com./inbox"), "mail.google.com");
}

#[test]
fn a_trailing_dot_does_not_slip_past_never_analyse() {
    let mut m = serde_json::Map::new();
    m.insert("type".into(), "tts.speak".into());
    m.insert("url".into(), "https://mail.google.com./mail/u/0".into());
    assert!(kairos_core::privacy::refusal(&m, &["mail.google.com".into()]).is_some());
}
