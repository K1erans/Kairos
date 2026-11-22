use crate::id::Id;
use crate::models::{AppData, AppGroup, Mode, Settings};
use crate::time::{self, add, Date};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Lower-cases and strips scheme, user info, `www.`, a leading `*.`, path, query, port and one
/// trailing dot from something a user typed. A bracketed IPv6 address (`http://[::1]:8080/`)
/// keeps the address without its brackets.
///
/// Unicode hostnames are lower-cased but not converted to punycode: no IDNA crate is a direct
/// dependency of kairos-core. Browsers report hosts in punycode, so a Unicode entry such as
/// `bücher.de` does not match `xn--bcher-kva.de` here (Kairos Shield converts its own copy).
pub fn normalize_site(raw: &str) -> String {
    let mut s = raw.trim().to_lowercase();
    if let Some(i) = s.find("://") {
        s = s[i + 3..].to_string();
    }
    if let Some(i) = s.find(|c| c == '/' || c == '?' || c == '#') {
        s.truncate(i);
    }
    if let Some(at) = s.rfind('@') {
        s = s[at + 1..].to_string();
    }
    if let Some(rest) = s.strip_prefix('[') {
        // [v6]:port
        s = rest.split(']').next().unwrap_or("").to_string();
    } else if s.matches(':').count() == 1 {
        // host:port. More than one colon is a bare IPv6 address, which has no port.
        if let Some(i) = s.find(':') {
            s.truncate(i);
        }
    }
    if let Some(rest) = s.strip_suffix('.') {
        s = rest.to_string();
    }
    if let Some(rest) = s.strip_prefix("*.") {
        s = rest.to_string();
    }
    if let Some(rest) = s.strip_prefix("www.") {
        s = rest.to_string();
    }
    s
}

/// Two-label public suffixes that are not one site, so an "Allow 5 min" for one of them would
/// let through every site under it. Not the full public suffix list, only the common ones.
const SHARED_SUFFIXES: &[&str] = &[
    "co.uk", "org.uk", "ac.uk", "gov.uk", "me.uk", "ltd.uk", "plc.uk", "net.uk", "com.au", "net.au", "org.au", "edu.au",
    "gov.au", "co.nz", "org.nz", "net.nz", "co.jp", "ne.jp", "or.jp", "ac.jp", "co.in", "net.in", "org.in", "co.za",
    "org.za", "com.br", "net.br", "org.br", "com.cn", "net.cn", "org.cn", "com.mx", "com.ar", "com.tr", "com.tw",
    "com.hk", "com.sg", "co.kr", "or.kr", "co.id", "com.my", "com.ph", "com.pk", "com.ua", "co.il", "github.io",
    "blogspot.com", "herokuapp.com", "vercel.app", "netlify.app", "pages.dev", "workers.dev", "appspot.com",
];

/// A desktop file id, as app blocks and app overrides name an app.
pub fn is_app_target(target: &str) -> bool {
    target.ends_with(".desktop")
}

/// Whether `target` names one site precisely enough to grant or match an override for it and
/// its subdomains: it has a dot (`com` or `localhost` would cover too much or nothing real), its
/// labels are not empty, and it is not a shared public suffix such as `co.uk`. An IPv6 address
/// counts as one site.
pub fn is_site_target(target: &str) -> bool {
    let host = normalize_site(target);
    if host.is_empty() || is_app_target(&host) {
        return false;
    }
    if host.contains(':') {
        return true;
    }
    host.contains('.') && host.split('.').all(|l| !l.is_empty()) && !SHARED_SUFFIXES.contains(&host.as_str())
}

/// Whether a grant or a record for `key` covers `target`. App ids match exactly; a site covers
/// its subdomains, but only when it names one site (see `is_site_target`).
pub fn target_matches(target: &str, key: &str) -> bool {
    if target == key {
        return true;
    }
    if is_app_target(target) || is_app_target(key) || !is_site_target(key) {
        return false;
    }
    host_matches(target, key)
}

/// A site matches when the hostname equals the entry or is a subdomain of it.
pub fn host_matches(host: &str, entry: &str) -> bool {
    let h = normalize_site(host);
    let e = normalize_site(entry);
    if e.is_empty() {
        return false;
    }
    h == e || h.ends_with(&format!(".{e}"))
}

pub fn host_in(host: &str, list: &[String]) -> bool {
    list.iter().any(|e| host_matches(host, e))
}

/// The host of a URL, as `URL(string:)?.host` gives it, normalized.
pub fn host_of(url: &str) -> String {
    let trimmed = url.trim();
    if let Some(i) = trimmed.find("://") {
        let rest = &trimmed[i + 3..];
        let end = rest.find(|c| c == '/' || c == '?' || c == '#').unwrap_or(rest.len());
        let mut authority = &rest[..end];
        if let Some(at) = authority.rfind('@') {
            authority = &authority[at + 1..];
        }
        if !authority.is_empty() {
            return normalize_site(authority);
        }
    }
    normalize_site(trimmed)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Strict,
    Exhausted,
    NoReason,
    NotBlocking,
    /// The target is not something Kairos blocks one at a time: a bare suffix such as `com`, or
    /// an app or site that is not blocked right now.
    NotBlocked,
}

impl Refusal {
    pub fn message(self) -> &'static str {
        match self {
            Refusal::Strict => "Strict mode has no overrides until the session ends.",
            Refusal::Exhausted => "You have used all your overrides for this session.",
            Refusal::NoReason => "Say why you need it first.",
            Refusal::NotBlocking => "Nothing is being blocked right now.",
            Refusal::NotBlocked => "That isn\u{2019}t blocked right now.",
        }
    }
}

/// Temporary "Allow 5 min" grants, limited per focus session.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct OverrideLedger {
    used: i32,
    /// target (hostname or desktop id) -> expiry
    allows: BTreeMap<String, Date>,
}

impl OverrideLedger {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn used(&self) -> i32 {
        self.used
    }
    pub fn remaining(&self, max: i32) -> i32 {
        (max - self.used).max(0)
    }

    pub fn request(&mut self, target: &str, reason: &str, mode: Mode, max: i32, minutes: i32, now: Date) -> Result<Date, Refusal> {
        if mode == Mode::Strict {
            return Err(Refusal::Strict);
        }
        if mode != Mode::Focus {
            return Err(Refusal::NotBlocking);
        }
        if reason.trim().is_empty() {
            return Err(Refusal::NoReason);
        }
        if !is_app_target(target) && !is_site_target(target) {
            return Err(Refusal::NotBlocked);
        }
        if self.used >= max {
            return Err(Refusal::Exhausted);
        }
        self.used += 1;
        let until = add(now, (minutes * 60) as f64);
        self.allows.insert(target.to_string(), until);
        Ok(until)
    }

    pub fn is_allowed(&self, target: &str, now: Date) -> bool {
        if let Some(until) = self.allows.get(target) {
            if *until > now {
                return true;
            }
        }
        // Site grants also cover subdomains; app grants cover only that app.
        self.allows.iter().any(|(k, v)| *v > now && target_matches(target, k))
    }

    pub fn active(&self, now: Date) -> BTreeMap<String, Date> {
        self.allows.iter().filter(|(_, v)| **v > now).map(|(k, v)| (k.clone(), *v)).collect()
    }

    pub fn next_expiry(&self, now: Date) -> Option<Date> {
        self.allows.values().filter(|v| **v > now).min().copied()
    }

    /// Called when a new focus session starts.
    pub fn reset(&mut self) {
        self.used = 0;
        self.allows.clear();
    }
}

/// The `state` object pushed to the extensions (see packages/shared/PROTOCOL.md).
pub fn bridge_state(data: &AppData, ledger: &OverrideLedger, cloud_voice: bool, now: Date) -> Value {
    let group = data.active_group();
    let task = data.current_task();
    let allows: Map<String, Value> = ledger.active(now).into_iter().map(|(k, v)| (k, json!(time::ms(v)))).collect();
    let rules = json!({
        "blockSites": group.map(|g| g.block_sites.clone()).unwrap_or_default(),
        "allowSites": group.map(|g| g.allow_sites.clone()).unwrap_or_default(),
        "hiddenElements": group.map(|g| g.hidden_elements.clone()).unwrap_or_default(),
        "jevEnabled": group.map(|g| g.jev_enabled).unwrap_or(false),
        "jevThreshold": group.map(|g| g.jev_threshold).unwrap_or(0.7),
        "neverAnalyse": data.settings.never_analyse,
        "jevFailClosed": data.settings.jev_fail_closed,
        "temporaryAllows": allows,
    });
    let overrides_left = if data.focus.mode == Mode::Focus { ledger.remaining(data.settings.max_overrides_per_session) } else { 0 };
    json!({
        "mode": data.focus.mode.raw(),
        "activeGroupId": group.map(|g| g.id.string()).unwrap_or_default(),
        "groupName": group.map(|g| g.name.clone()).unwrap_or_default(),
        "groupColor": group.map(|g| g.color_hex.clone()).unwrap_or_else(|| "#4F7CFF".into()),
        "currentTask": task.map(|t| json!({"id": t.id.string(), "title": t.title})).unwrap_or(Value::Null),
        "endsAt": data.focus.ends_at.map(|d| json!(time::ms(d))).unwrap_or(Value::Null),
        "overridesLeft": overrides_left,
        "overrideMinutes": data.settings.override_minutes,
        "strictUntil": data.focus.strict_until.map(|d| json!(time::ms(d))).unwrap_or(Value::Null),
        "cloudVoice": if cloud_voice {
            json!({"provider": data.settings.voice_provider.raw(), "name": data.settings.voice_provider.label()})
        } else { Value::Null },
        "rules": rules,
    })
}

/// The two site lists of a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteList {
    Block,
    Allow,
}

impl SiteList {
    pub fn parse(raw: &str) -> Option<SiteList> {
        match raw {
            "block" => Some(SiteList::Block),
            "allow" => Some(SiteList::Allow),
            _ => None,
        }
    }
}

fn subset(a: &[String], b: &[String]) -> bool {
    let b: BTreeSet<&String> = b.iter().collect();
    a.iter().all(|x| b.contains(x))
}

/// While a strict focus phase is locked, the active group's rules can block more but never less.
pub mod rule_lock {
    use super::*;

    /// Whether going from `old` to `new` lets through anything `old` stopped. Jev does not judge
    /// apps pinned to the group's Kairos Bar, so pinning a new app loosens while Jev is on;
    /// unpinning or reordering does not.
    pub fn loosens(old: &AppGroup, new: &AppGroup) -> bool {
        !subset(&old.block_sites, &new.block_sites)
            || (old.jev_enabled && !subset(&new.dock_apps, &old.dock_apps))
            || !subset(&new.allow_sites, &old.allow_sites)
            || !subset(&old.blocked_apps, &new.blocked_apps)
            || old
                .hidden_elements
                .iter()
                .any(|(host, sels)| !subset(sels, new.hidden_elements.get(host).map(|v| v.as_slice()).unwrap_or(&[])))
            || (old.jev_enabled && (!new.jev_enabled || new.jev_threshold > old.jev_threshold))
    }

    /// Whether going from `old` to `new` Settings lets through anything `old` stopped: browsers
    /// without Kairos Shield, pages Jev can't judge, or sites taken out of Jev's checks.
    pub fn settings_loosen(old: &Settings, new: &Settings) -> bool {
        (old.block_unshielded_browsers && !new.block_unshielded_browsers)
            || (old.jev_fail_closed && !new.jev_fail_closed)
            || !subset(&new.never_analyse, &old.never_analyse)
    }

    /// The line shown where the loosening controls are while the lock holds.
    pub fn note(until: Option<Date>) -> String {
        match until {
            None => "Strict focus: you can block more, but nothing can be unblocked or allowed until this session ends.".into(),
            Some(u) => format!(
                "Strict focus until {}: you can block more, but nothing can be unblocked or allowed until then.",
                time::short_time(u)
            ),
        }
    }
}

impl AppData {
    /// Whether `target` (a desktop file id, or a hostname as `normalize_site` gives it) is blocked
    /// right now, or could be by Jev, in the active group. "Allow 5 min" should only be granted
    /// for such a target: `OverrideLedger::request` refuses bare suffixes itself but cannot see
    /// the group. `shield_connected` is whether Kairos Shield is connected, which decides whether
    /// a Chromium browser is blocked for running without it.
    pub fn is_blocked_target(&self, target: &str, shield_connected: bool) -> bool {
        let Some(group) = self.active_group() else { return false };
        if !self.focus.mode.blocks() {
            return false;
        }
        if is_app_target(target) {
            let never = |_: &str| false;
            let ctx = crate::appguard::Context {
                mode: self.focus.mode,
                group: Some(group),
                own: Some("kairos.desktop"),
                shield_connected,
                blocks_unshielded_browsers: self.settings.block_unshielded_browsers,
                is_allowed: &never,
            };
            if ctx.reason(target).is_some() {
                return true;
            }
            // Jev judges apps that are on no list and not pinned to Kairos Bar.
            return group.jev_enabled
                && !group.dock_apps.iter().any(|a| a == target)
                && !crate::appguard::never_guard(target)
                && target != "kairos.desktop";
        }
        if !is_site_target(target) {
            return false;
        }
        let host = normalize_site(target);
        if host_in(&host, &group.allow_sites) {
            return false;
        }
        host_in(&host, &group.block_sites) || (group.jev_enabled && !host_in(&host, &self.settings.never_analyse))
    }

    /// Every change to a group's rules goes through here. While a strict focus phase is locked a
    /// change to the active group that loosens its rules is refused as a whole. Returns false
    /// when refused or when nothing changed.
    pub fn update_group(&mut self, id: Id, strict_locked: bool, change: impl FnOnce(&mut AppGroup)) -> bool {
        let Some(i) = self.groups.iter().position(|g| g.id == id) else { return false };
        let mut g = self.groups[i].clone();
        change(&mut g);
        let locked = strict_locked && Some(id) == self.active_group().map(|g| g.id);
        if g == self.groups[i] || (locked && rule_lock::loosens(&self.groups[i], &g)) {
            return false;
        }
        self.groups[i] = g;
        true
    }

    /// Every change to Settings goes through here. While a strict focus phase is locked a change
    /// that loosens blocking is refused as a whole. Returns false when refused or unchanged.
    pub fn update_settings(&mut self, strict_locked: bool, change: impl FnOnce(&mut Settings)) -> bool {
        let mut next = self.settings.clone();
        change(&mut next);
        if next == self.settings || (strict_locked && rule_lock::settings_loosen(&self.settings, &next)) {
            return false;
        }
        self.settings = next;
        true
    }

    /// `rule.addSite`: the site goes on one list of the active group and off the other.
    pub fn add_site(&mut self, raw: &str, list: SiteList, strict_locked: bool) -> bool {
        let site = normalize_site(raw);
        let Some(id) = self.active_group().map(|g| g.id) else { return false };
        if site.is_empty() {
            return false;
        }
        self.update_group(id, strict_locked, |g| match list {
            SiteList::Block => {
                g.allow_sites.retain(|s| *s != site);
                if !g.block_sites.contains(&site) {
                    g.block_sites.push(site.clone());
                }
            }
            SiteList::Allow => {
                g.block_sites.retain(|s| *s != site);
                if !g.allow_sites.contains(&site) {
                    g.allow_sites.push(site.clone());
                }
            }
        })
    }

    /// "Block in <group>" from Kairos Bar.
    pub fn set_app_blocked(&mut self, app: &str, blocked: bool, strict_locked: bool) -> bool {
        let Some(id) = self.active_group().map(|g| g.id) else { return false };
        self.update_group(id, strict_locked, |g| {
            if blocked {
                if !g.blocked_apps.iter().any(|a| a == app) {
                    g.blocked_apps.push(app.to_string());
                }
            } else {
                g.blocked_apps.retain(|a| a != app);
            }
        })
    }

    /// `rule.addElement` and `rule.removeElement` from Kairos Shield, on the active group.
    pub fn set_element_hidden(&mut self, selector: &str, raw_host: &str, hidden: bool, strict_locked: bool) -> bool {
        let host = normalize_site(raw_host);
        let Some(id) = self.active_group().map(|g| g.id) else { return false };
        if host.is_empty() || selector.is_empty() {
            return false;
        }
        self.update_group(id, strict_locked, |g| {
            if hidden {
                g.hide_element(selector, &host)
            } else {
                g.show_element(selector, &host)
            }
        })
    }
}
