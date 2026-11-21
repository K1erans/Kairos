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
    let _ = ledger.request("a", "r", Mode::Focus, 3, 5, now);
    let _ = ledger.request("b", "r", Mode::Focus, 3, 5, plus(now, 60.0));
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
        ("changing the apps on Kairos Bar", Box::new(|g| g.dock_apps = vec!["firefox.desktop".into()])),
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
