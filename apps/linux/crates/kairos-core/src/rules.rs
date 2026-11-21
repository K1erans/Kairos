use crate::id::Id;
use crate::models::{AppData, AppGroup, Mode};
use crate::time::{self, add, Date};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Lower-cases and strips scheme, `www.`, path and port from something a user typed.
pub fn normalize_site(raw: &str) -> String {
    let mut s = raw.trim().to_lowercase();
    if let Some(i) = s.find("://") {
        s = s[i + 3..].to_string();
    }
    if let Some(i) = s.find('/') {
        s.truncate(i);
    }
    if let Some(i) = s.find(':') {
        s.truncate(i);
    }
    if let Some(rest) = s.strip_prefix("www.") {
        s = rest.to_string();
    }
    s
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
}

impl Refusal {
    pub fn message(self) -> &'static str {
        match self {
            Refusal::Strict => "Strict mode has no overrides until the session ends.",
            Refusal::Exhausted => "You have used all your overrides for this session.",
            Refusal::NoReason => "Say why you need it first.",
            Refusal::NotBlocking => "Nothing is being blocked right now.",
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
        // Site grants also cover subdomains.
        self.allows.iter().any(|(k, v)| *v > now && host_matches(target, k))
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

    /// Whether going from `old` to `new` lets through anything `old` stopped.
    pub fn loosens(old: &AppGroup, new: &AppGroup) -> bool {
        !subset(&old.block_sites, &new.block_sites)
            || !subset(&new.allow_sites, &old.allow_sites)
            || !subset(&old.blocked_apps, &new.blocked_apps)
            || old
                .hidden_elements
                .iter()
                .any(|(host, sels)| !subset(sels, new.hidden_elements.get(host).map(|v| v.as_slice()).unwrap_or(&[])))
            || (old.jev_enabled && (!new.jev_enabled || new.jev_threshold > old.jev_threshold))
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
