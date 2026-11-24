//! Kairos Bar's geometry and motion. Positions are measured along the bar (the axis, 0 at the
//! leading end of the bar at rest) and across it (from the screen edge inward). Pure, so
//! magnification, auto-hide and the pointer's hit zone are testable without a screen.

use crate::id::Id;
use crate::models::{AppData, DockEdge, SwitchBehavior};
use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DockFolder {
    Downloads,
    Trash,
}

impl DockFolder {
    pub const ALL: [DockFolder; 2] = [DockFolder::Downloads, DockFolder::Trash];
    pub fn name(self) -> &'static str {
        match self {
            DockFolder::Downloads => "Downloads",
            DockFolder::Trash => "Trash",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    /// Opens Kairos's own window, which otherwise only the tray icon in Cinnamon's panel does.
    Kairos,
    Group(Id),
    NewGroup,
    Divider,
    App(String),
    AddApp,
    Separator,
    /// An open app that is not pinned to the active group.
    Running(String),
    Folder(DockFolder),
}

impl Slot {
    pub fn magnifies(&self) -> bool {
        !matches!(self, Slot::Divider | Slot::Separator)
    }
    pub fn app_id(&self) -> Option<&str> {
        match self {
            Slot::App(a) | Slot::Running(a) => Some(a),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub slot: Slot,
    pub center: f64,
    pub size: f64,
    pub scale: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub items: Vec<Placement>,
    pub start: f64,
    pub end: f64,
    /// The magnifying slot under the pointer, which gets the name label.
    pub hovered: Option<usize>,
}

pub const MAGNIFICATION: f64 = 1.6;
pub const DIVIDER_LENGTH: f64 = 11.0;

#[derive(Clone, Debug, PartialEq)]
pub struct DockGeometry {
    pub slots: Vec<Slot>,
    pub icon: f64,
    pub max_scale: f64,
}

impl DockGeometry {
    pub fn new(slots: Vec<Slot>, icon: f64, max_scale: f64) -> Self {
        DockGeometry { slots, icon, max_scale }
    }

    /// Kairos itself comes first, then the groups. `running` is the open apps in launch order,
    /// and `hidden` are the apps Kairos hid on a group switch. Open apps not pinned to the active
    /// group follow the pinned ones after a separator, excluding blocked apps hidden by a group
    /// switch and (when hiding other apps on switch) apps that belong to another group — so apps
    /// not in any group always show on the bar. Downloads and Trash come last, after another
    /// separator, unless turned off in Settings.
    pub fn slots_for(data: &AppData, running: &[String], hidden: &HashSet<String>) -> Vec<Slot> {
        let apps: Vec<String> = data.active_group().map(|g| g.dock_apps.clone()).unwrap_or_default();
        let mut seen: HashSet<String> = apps.iter().cloned().collect();
        let active_id = data.active_group().map(|g| g.id);
        let behavior = data.active_switch_behavior();
        let hides_other_groups = active_id.is_some() && behavior == SwitchBehavior::Hide;
        let other_group_apps: HashSet<&str> = if hides_other_groups {
            data.groups
                .iter()
                .filter(|g| Some(g.id) != active_id)
                .flat_map(|g| g.dock_apps.iter().map(String::as_str))
                .collect()
        } else {
            HashSet::new()
        };
        let blocked_apps: HashSet<&str> = if behavior == SwitchBehavior::HideBlocked {
            data.active_group()
                .map(|g| g.blocked_apps.iter().map(String::as_str).collect())
                .unwrap_or_default()
        } else {
            HashSet::new()
        };
        let extra: Vec<String> = running
            .iter()
            .filter(|r| {
                !other_group_apps.contains(r.as_str())
                    && !(blocked_apps.contains(r.as_str()) && hidden.contains(*r))
                    && seen.insert((*r).clone())
            })
            .cloned()
            .collect();
        let mut slots = vec![Slot::Kairos, Slot::Separator];
        slots.extend(data.groups.iter().map(|g| Slot::Group(g.id)));
        slots.push(Slot::NewGroup);
        slots.push(Slot::Divider);
        if apps.is_empty() {
            slots.push(Slot::AddApp);
        } else {
            slots.extend(apps.into_iter().map(Slot::App));
        }
        if !extra.is_empty() {
            slots.push(Slot::Separator);
            slots.extend(extra.into_iter().map(Slot::Running));
        }
        if data.settings.shows_dock_folders {
            slots.push(Slot::Separator);
            slots.extend(DockFolder::ALL.iter().map(|f| Slot::Folder(*f)));
        }
        slots
    }

    pub fn gap(&self) -> f64 {
        (self.icon * 0.14).round().max(3.0)
    }
    pub fn padding(&self) -> f64 {
        (self.icon * 0.2).round().max(7.0)
    }
    /// How far from the pointer, along the axis, an icon still grows.
    pub fn range(&self) -> f64 {
        self.icon * 2.5
    }
    pub fn thickness(&self) -> f64 {
        self.icon + 2.0 * self.padding()
    }
    pub fn corner_radius(&self) -> f64 {
        (self.thickness() / 3.0).round()
    }
    /// How far a magnified icon reaches past the bar, away from the screen edge.
    pub fn headroom(&self) -> f64 {
        (self.max_scale - 1.0) * self.icon
    }
    pub fn base_length(&self, slot: &Slot) -> f64 {
        if slot.magnifies() {
            self.icon
        } else {
            DIVIDER_LENGTH
        }
    }
    pub fn rest_length(&self) -> f64 {
        2.0 * self.padding()
            + self.slots.iter().map(|s| self.base_length(s)).sum::<f64>()
            + self.gap() * (self.slots.len().saturating_sub(1)) as f64
    }

    pub fn scale(&self, distance: f64) -> f64 {
        let d = distance.abs().min(self.range());
        1.0 + (self.max_scale - 1.0) * ((PI * d / self.range()).cos() + 1.0) / 2.0
    }

    pub fn slot_at(&self, axis: f64) -> Option<Slot> {
        let l = self.layout(Some(axis), 1.0);
        l.hovered.map(|i| l.items[i].slot.clone())
    }

    fn spans(&self) -> Vec<(f64, f64)> {
        let mut x = self.padding();
        let gap = self.gap();
        self.slots
            .iter()
            .map(|s| {
                let len = self.base_length(s);
                let span = (x, x + len);
                x += len + gap;
                span
            })
            .collect()
    }

    /// Lays the slots out for a pointer at `pointer` along the axis, or at rest for None. The
    /// bar is stretched like a lens around the pointer, and the point under the pointer stays
    /// exactly under it, so nothing overlaps and the ends move smoothly.
    pub fn layout(&self, pointer: Option<f64>, amount: f64) -> Layout {
        let rest = self.spans();
        let p = match pointer {
            Some(p) if !self.slots.is_empty() => p,
            _ => {
                let items = self
                    .slots
                    .iter()
                    .zip(&rest)
                    .map(|(s, (lo, hi))| Placement { slot: s.clone(), center: (lo + hi) / 2.0, size: hi - lo, scale: 1.0 })
                    .collect();
                return Layout { items, start: 0.0, end: self.rest_length(), hovered: None };
            }
        };
        let k = self.stretch() * amount;
        let range = self.range();
        let at = |x: f64| x + k * (self.bulge(x - p) - range / 2.0);
        let items: Vec<Placement> = self
            .slots
            .iter()
            .zip(&rest)
            .map(|(s, (lo, hi))| {
                let (lo, hi) = (at(*lo), at(*hi));
                let size = if s.magnifies() { hi - lo } else { self.base_length(s) };
                Placement { slot: s.clone(), center: (lo + hi) / 2.0, size, scale: size / self.base_length(s) }
            })
            .collect();
        let half = self.gap() / 2.0;
        let hovered = items.iter().position(|i| i.slot.magnifies() && (p - i.center).abs() <= i.size / 2.0 + half);
        Layout { items, start: at(0.0), end: at(self.rest_length()), hovered }
    }

    fn bulge(&self, u: f64) -> f64 {
        let range = self.range();
        if range <= 0.0 {
            return 0.0;
        }
        let v = u.clamp(-range, range);
        (v + range + range / PI * (PI * v / range).sin()) / 2.0
    }

    fn stretch(&self) -> f64 {
        let range = self.range();
        if range <= 0.0 {
            return 0.0;
        }
        let a = PI * self.icon / 2.0 / range;
        (self.max_scale - 1.0) / ((1.0 + a.sin() / a) / 2.0)
    }

    /// How far the bar can reach past its rest extent at each end, over every pointer position.
    pub fn overflow(&self) -> (f64, f64) {
        let (mut lead, mut trail) = (0.0f64, 0.0f64);
        let rest = self.rest_length();
        let mut p = -self.range();
        while p <= rest + self.range() {
            let l = self.layout(Some(p), 1.0);
            lead = lead.max(-l.start);
            trail = trail.max(l.end - rest);
            p += 1.0;
        }
        (lead.ceil(), trail.ceil())
    }

    /// Whether a pointer at (axis, cross) is on the bar. It takes the bar at rest to engage, and
    /// once engaged the zone grows to cover the magnified icons.
    pub fn engages(&self, axis: f64, cross: f64, inset: f64, engaged: bool) -> bool {
        if cross < -1.0 {
            return false;
        }
        if !engaged {
            return cross <= inset + self.thickness() && axis >= 0.0 && axis <= self.rest_length();
        }
        let l = self.layout(Some(axis), 1.0);
        cross <= inset + self.thickness() + self.headroom() && axis >= l.start && axis <= l.end
    }

    /// Whether a pointer at `axis` along the bar and `from_edge` in from the screen edge is in the
    /// strip that calls a hidden bar back.
    pub fn touches_edge(&self, axis: f64, from_edge: f64) -> bool {
        from_edge <= EDGE_STRIP && axis >= 0.0 && axis <= self.rest_length()
    }
}

/// Items keyed so that repeated slots (separators) stay unique.
pub fn keyed(items: &[Placement]) -> Vec<((Slot, usize), &Placement)> {
    let mut seen: HashMap<Slot, usize> = HashMap::new();
    items
        .iter()
        .map(|i| {
            let n = seen.entry(i.slot.clone()).or_insert(0);
            let key = (i.slot.clone(), *n);
            *n += 1;
            (key, i)
        })
        .collect()
}

/// How Kairos Bar moves, from GTK's "enable animations" setting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockMotion {
    pub reduced: bool,
}

impl DockMotion {
    pub fn max_scale(&self) -> f64 {
        if self.reduced {
            1.0
        } else {
            MAGNIFICATION
        }
    }
    /// For `reveal` (0 hidden, 1 shown): the share of the slide still to go past the screen
    /// edge, and the bar's opacity.
    pub fn appearance(&self, reveal: f64) -> (f64, f64) {
        if self.reduced {
            (0.0, reveal)
        } else {
            (1.0 - reveal, 1.0)
        }
    }
}

// MARK: Auto-hide

pub const REVEAL_DELAY: f64 = 0.2;
pub const HIDE_DELAY: f64 = 0.5;
pub const SLIDE: f64 = 0.3;
pub const EDGE_STRIP: f64 = 2.0;
pub const SECOND_PUSH_PAUSE: f64 = 0.15;

/// Times are seconds on a monotonic clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HideState {
    Hidden { armed_at: Option<f64> },
    Revealing { from: f64, at: f64 },
    Shown { left_at: Option<f64> },
    Hiding { from: f64, at: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HideRule {
    Pinned,
    AutoHide,
    FullScreen,
}

impl HideRule {
    pub fn new(auto_hide: bool, hides_in_full_screen: bool, full_screen: bool) -> Self {
        if full_screen && (auto_hide || hides_in_full_screen) {
            HideRule::FullScreen
        } else if auto_hide {
            HideRule::AutoHide
        } else {
            HideRule::Pinned
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct HideInputs {
    pub at_edge: bool,
    pub on_bar: bool,
    pub held: bool,
    pub pushed: bool,
}

impl HideInputs {
    fn keeps_out(&self) -> bool {
        self.at_edge || self.on_bar || self.held
    }
}

/// When Kairos Bar is on screen, after the macOS Dock: pinned it is always out; with auto-hide
/// it slides off shortly after the pointer leaves and back after the pointer rests on the edge;
/// in a full-screen window it is hidden and only a second push against the edge brings it out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockAutoHide {
    pub state: HideState,
    pub rule: HideRule,
}

impl DockAutoHide {
    pub fn new(rule: HideRule, now: f64) -> Self {
        let state = if rule == HideRule::FullScreen {
            HideState::Hidden { armed_at: None }
        } else {
            HideState::Shown { left_at: if rule == HideRule::Pinned { None } else { Some(now) } }
        };
        DockAutoHide { state, rule }
    }

    pub fn update(&mut self, inputs: HideInputs, rule: Option<HideRule>, now: f64) {
        if let Some(next) = rule {
            if next != self.rule {
                self.follow(next, inputs);
            }
        }
        if self.rule == HideRule::Pinned {
            self.state = HideState::Shown { left_at: None };
            return;
        }
        match self.state {
            HideState::Hidden { armed_at } => {
                if !inputs.at_edge {
                    self.state = HideState::Hidden { armed_at: None };
                    return;
                }
                if self.rule == HideRule::FullScreen {
                    match armed_at {
                        Some(armed) if inputs.pushed => {
                            self.state = if now - armed >= SECOND_PUSH_PAUSE {
                                HideState::Revealing { from: 0.0, at: now }
                            } else {
                                HideState::Hidden { armed_at: Some(now) }
                            };
                        }
                        _ => self.state = HideState::Hidden { armed_at: Some(armed_at.unwrap_or(now)) },
                    }
                    return;
                }
                let armed = armed_at.unwrap_or(now);
                self.state = if now - armed >= REVEAL_DELAY {
                    HideState::Revealing { from: 0.0, at: now }
                } else {
                    HideState::Hidden { armed_at: Some(armed) }
                };
            }
            HideState::Revealing { .. } => {
                if self.progress(now) >= 1.0 {
                    self.state = HideState::Shown { left_at: if inputs.keeps_out() { None } else { Some(now) } };
                }
            }
            HideState::Shown { left_at } => {
                if inputs.keeps_out() {
                    self.state = HideState::Shown { left_at: None };
                    return;
                }
                let left = left_at.unwrap_or(now);
                self.state = if now - left >= HIDE_DELAY {
                    HideState::Hiding { from: 1.0, at: now }
                } else {
                    HideState::Shown { left_at: Some(left) }
                };
            }
            HideState::Hiding { .. } => {
                let p = self.progress(now);
                if inputs.keeps_out() {
                    self.state = HideState::Revealing { from: p, at: now };
                } else if p <= 0.0 {
                    self.state = HideState::Hidden { armed_at: None };
                }
            }
        }
    }

    fn follow(&mut self, next: HideRule, inputs: HideInputs) {
        match next {
            HideRule::Pinned => self.state = HideState::Shown { left_at: None },
            HideRule::AutoHide => {}
            HideRule::FullScreen => {
                if !(inputs.on_bar || inputs.held) {
                    self.state = HideState::Hidden { armed_at: None };
                }
            }
        }
        self.rule = next;
    }

    pub fn progress(&self, now: f64) -> f64 {
        let p = match self.state {
            HideState::Hidden { .. } => return 0.0,
            HideState::Shown { .. } => return 1.0,
            HideState::Revealing { from, at } => from + (now - at) / SLIDE,
            HideState::Hiding { from, at } => from - (now - at) / SLIDE,
        };
        if p >= 1.0 - 1e-6 {
            1.0
        } else if p <= 1e-6 {
            0.0
        } else {
            p
        }
    }

    /// How much of the bar is on screen, eased in and out.
    pub fn visible(&self, now: f64) -> f64 {
        let p = self.progress(now);
        p * p * (3.0 - 2.0 * p)
    }

    pub fn is_out(&self) -> bool {
        matches!(self.state, HideState::Revealing { .. } | HideState::Shown { .. })
    }

    pub fn is_sliding(&self) -> bool {
        matches!(self.state, HideState::Revealing { .. } | HideState::Hiding { .. })
    }

    /// When `update` must run again even if the pointer does not move.
    pub fn next_decision(&self) -> Option<f64> {
        match self.state {
            HideState::Hidden { armed_at } => {
                if self.rule == HideRule::AutoHide {
                    armed_at.map(|a| a + REVEAL_DELAY)
                } else {
                    None
                }
            }
            HideState::Revealing { from, at } => Some(at + (1.0 - from) * SLIDE),
            HideState::Shown { left_at } => {
                if self.rule == HideRule::Pinned {
                    None
                } else {
                    left_at.map(|l| l + HIDE_DELAY)
                }
            }
            HideState::Hiding { from, at } => Some(at + from * SLIDE),
        }
    }
}

// MARK: Labels

pub mod label {
    pub const MARGIN: f64 = 4.0;
    pub const GAP: f64 = 10.0;

    /// The label's centre along the bar: over the item when it fits, otherwise moved just far
    /// enough to stay `MARGIN` inside `bounds`.
    pub fn center(item: f64, width: f64, lo_bound: f64, hi_bound: f64) -> f64 {
        let lo = lo_bound + MARGIN + width / 2.0;
        let hi = hi_bound - MARGIN - width / 2.0;
        if lo > hi {
            return (lo_bound + hi_bound) / 2.0;
        }
        item.clamp(lo, hi)
    }

    pub fn growth(length: f64, widest: f64) -> f64 {
        ((widest + 2.0 * MARGIN - length) / 2.0).max(0.0).ceil()
    }

    pub fn side_lane(widest: f64) -> f64 {
        (widest + GAP + MARGIN).ceil()
    }
}

// MARK: Items

/// What files dropped on Kairos Bar do, by what is under the pointer.
#[derive(Clone, Debug, PartialEq)]
pub enum DockDrop {
    AddApps(Vec<String>),
    Open(Vec<PathBuf>, String),
    MoveToTrash(Vec<PathBuf>),
    Nothing,
}

impl DockDrop {
    /// `app_id` names the app a .desktop file is, or None for anything else.
    pub fn plan(paths: &[PathBuf], on: Option<&Slot>, app_id: &dyn Fn(&PathBuf) -> Option<String>) -> DockDrop {
        if paths.is_empty() {
            return DockDrop::Nothing;
        }
        if on == Some(&Slot::Folder(DockFolder::Trash)) {
            return DockDrop::MoveToTrash(paths.to_vec());
        }
        let is_app = |p: &PathBuf| p.extension().map(|e| e == "desktop").unwrap_or(false);
        let apps: Vec<String> = paths.iter().filter(|p| is_app(p)).filter_map(app_id).collect();
        let documents: Vec<PathBuf> = paths.iter().filter(|p| !is_app(p)).cloned().collect();
        if let Some(Slot::App(id) | Slot::Running(id)) = on {
            if !documents.is_empty() {
                return DockDrop::Open(documents, id.clone());
            }
        }
        if apps.is_empty() {
            DockDrop::Nothing
        } else {
            DockDrop::AddApps(apps)
        }
    }
}

/// Apps started from Kairos Bar that are still launching, for the pulse on their icons.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct DockLaunches {
    started: HashMap<String, f64>,
}

impl DockLaunches {
    pub const TIMEOUT: f64 = 10.0;
    pub fn ids(&self) -> HashSet<String> {
        self.started.keys().cloned().collect()
    }
    pub fn contains(&self, id: &str) -> bool {
        self.started.contains_key(id)
    }
    pub fn start(&mut self, id: &str, now: f64) {
        self.started.insert(id.to_string(), now);
    }
    pub fn update(&mut self, running: &HashSet<String>, now: f64) {
        self.started.retain(|k, v| !running.contains(k) && now - *v < Self::TIMEOUT);
    }
    pub fn is_empty(&self) -> bool {
        self.started.is_empty()
    }
}

/// The list with `element` moved `offset` places, stopping at either end.
pub fn moving_by<T: PartialEq + Clone>(list: &[T], element: &T, offset: i64) -> Vec<T> {
    let Some(i) = list.iter().position(|e| e == element) else { return list.to_vec() };
    let mut copy = list.to_vec();
    let e = copy.remove(i);
    let to = (i as i64 + offset).clamp(0, copy.len() as i64) as usize;
    copy.insert(to, e);
    copy
}

/// The list with `element` moved to `index`, counted in the result.
pub fn moving_to<T: PartialEq + Clone>(list: &[T], element: &T, index: usize) -> Vec<T> {
    let Some(i) = list.iter().position(|e| e == element) else { return list.to_vec() };
    moving_by(list, element, index as i64 - i as i64)
}

pub mod reorder {
    /// The peer whose resting centre is nearest the pointer.
    pub fn index(centers: &[f64], pointer: f64) -> Option<usize> {
        (0..centers.len()).min_by(|a, b| {
            (centers[*a] - pointer).abs().partial_cmp(&(centers[*b] - pointer).abs()).unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Section {
        Pinned(usize),
        Open,
    }

    pub fn section(pointer: f64, pinned: &[f64], open: &[f64], icon: f64) -> Section {
        let last = pinned.iter().cloned().fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))));
        let first = open.iter().cloned().fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))));
        let boundary = match (last, first) {
            (Some(l), Some(f)) => (l + f) / 2.0,
            (Some(l), None) => l + icon * 0.75,
            (None, Some(f)) => f - icon * 0.75,
            (None, None) => return Section::Open,
        };
        if pointer < boundary {
            Section::Pinned(pinned.iter().filter(|c| **c < pointer).count())
        } else {
            Section::Open
        }
    }
}

// MARK: Placement

/// The panel's position on its monitor. `monitor` is the whole monitor and `work` the part the
/// window manager leaves free of panels (or the monitor itself while Kairos hides the panel).
pub fn panel_origin(edge: DockEdge, w: f64, h: f64, bar_mid: f64, monitor: crate::clearance::Rect, work: crate::clearance::Rect) -> (f64, f64) {
    let mid_y = work.y + work.h / 2.0;
    let (mut x, mut y) = match edge {
        DockEdge::Left => (work.x, mid_y - bar_mid),
        DockEdge::Right => (work.max_x() - w, mid_y - bar_mid),
        DockEdge::Bottom => (work.mid_x() - bar_mid, work.max_y() - h),
    };
    // As on the Mac: kept off the leading edge first, then off the trailing edge, so a panel
    // wider than the monitor keeps its trailing end on screen.
    if edge == DockEdge::Bottom {
        x = x.max(monitor.x).min(monitor.max_x() - w);
    } else {
        y = y.max(monitor.y).min(monitor.max_y() - h);
    }
    (x.round(), y.round())
}

/// The panel's frame and how to map between panel coordinates and bar coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct DockFrame {
    pub geometry: DockGeometry,
    pub edge: DockEdge,
    pub motion: DockMotion,
    pub lead: f64,
    pub trail: f64,
    pub label_width: f64,
}

pub const INSET: f64 = 4.0;

impl DockFrame {
    pub fn new(slots: Vec<Slot>, icon: f64, edge: DockEdge, reduced: bool, label_width: f64) -> Self {
        let motion = DockMotion { reduced };
        let geometry = DockGeometry::new(slots, icon, motion.max_scale());
        let (mut lead, mut trail) = geometry.overflow();
        if edge == DockEdge::Bottom {
            let grow = label::growth(lead + geometry.rest_length() + trail, label_width);
            lead += grow;
            trail += grow;
        }
        DockFrame { geometry, edge, motion, lead, trail, label_width }
    }

    pub fn label_lane(&self) -> f64 {
        if self.edge == DockEdge::Bottom {
            44.0
        } else {
            label::side_lane(self.label_width)
        }
    }
    pub fn axis_length(&self) -> f64 {
        self.lead + self.geometry.rest_length() + self.trail
    }
    pub fn cross_length(&self) -> f64 {
        INSET + self.geometry.thickness() + self.geometry.headroom() + self.label_lane()
    }
    pub fn size(&self) -> (f64, f64) {
        if self.edge == DockEdge::Bottom {
            (self.axis_length().ceil(), self.cross_length().ceil())
        } else {
            (self.cross_length().ceil(), self.axis_length().ceil())
        }
    }

    pub fn point(&self, axis: f64, cross: f64) -> (f64, f64) {
        let (w, h) = self.size();
        match self.edge {
            DockEdge::Left => (cross, self.lead + axis),
            DockEdge::Right => (w - cross, self.lead + axis),
            DockEdge::Bottom => (self.lead + axis, h - cross),
        }
    }

    pub fn bar(&self, x: f64, y: f64) -> (f64, f64) {
        let (w, h) = self.size();
        match self.edge {
            DockEdge::Left => (y - self.lead, x),
            DockEdge::Right => (y - self.lead, w - x),
            DockEdge::Bottom => (x - self.lead, h - y),
        }
    }

    /// A size given as (along the bar, across it), turned to the screen.
    pub fn oriented(&self, along: f64, across: f64) -> (f64, f64) {
        if self.edge == DockEdge::Bottom {
            (along, across)
        } else {
            (across, along)
        }
    }
}
