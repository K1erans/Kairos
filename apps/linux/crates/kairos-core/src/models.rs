//! Everything Kairos persists. Field names and enum values match the macOS app's JSON, so a
//! profile exported on one platform imports on the other. Apps are named by freedesktop desktop
//! file ids ("firefox.desktop") where the Mac uses bundle ids.

use crate::id::Id;
use crate::lenient;
use crate::time::{self, Date};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Off,
    Focus,
    Strict,
    #[serde(rename = "break")]
    OnBreak,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Off, Mode::Focus, Mode::Strict, Mode::OnBreak];
    pub fn label(self) -> &'static str {
        match self {
            Mode::Off => "Off",
            Mode::Focus => "Focus",
            Mode::OnBreak => "Break",
            Mode::Strict => "Strict",
        }
    }
    pub fn raw(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Focus => "focus",
            Mode::OnBreak => "break",
            Mode::Strict => "strict",
        }
    }
    /// Site rules, app rules and Jev apply only in these modes.
    pub fn blocks(self) -> bool {
        matches!(self, Mode::Focus | Mode::Strict)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SwitchBehavior {
    Leave,
    /// Hides only the apps the new group blocks.
    HideBlocked,
    /// Hides (minimises) every app that is not pinned to the group.
    #[default]
    Hide,
}

impl SwitchBehavior {
    pub const ALL: [SwitchBehavior; 3] = [SwitchBehavior::Leave, SwitchBehavior::HideBlocked, SwitchBehavior::Hide];
    pub fn label(self) -> &'static str {
        match self {
            SwitchBehavior::Leave => "Leave other apps alone",
            SwitchBehavior::HideBlocked => "Hide the apps that group blocks",
            SwitchBehavior::Hide => "Hide every app outside the group",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppGroup {
    pub id: Id,
    pub name: String,
    pub icon: String,
    pub color_hex: String,
    /// Desktop file ids shown in Kairos Bar.
    pub dock_apps: Vec<String>,
    pub blocked_apps: Vec<String>,
    pub allow_sites: Vec<String>,
    pub block_sites: Vec<String>,
    /// hostname -> CSS selectors hidden on that site.
    pub hidden_elements: BTreeMap<String, Vec<String>>,
    pub jev_enabled: bool,
    pub jev_threshold: f64,
    pub tempo_preset: String,
    /// Workspace (1-9) to switch to with this group. 0 leaves the workspace alone.
    pub desktop: i32,
}

impl AppGroup {
    pub fn new(name: &str) -> Self {
        AppGroup {
            id: Id::new(),
            name: name.to_string(),
            icon: "square.grid.2x2".into(),
            color_hex: "#4F7CFF".into(),
            dock_apps: vec![],
            blocked_apps: vec![],
            allow_sites: vec![],
            block_sites: vec![],
            hidden_elements: BTreeMap::new(),
            jev_enabled: true,
            jev_threshold: 0.7,
            tempo_preset: "Classic".into(),
            desktop: 0,
        }
    }

    pub fn with(name: &str, icon: &str, color: &str) -> Self {
        let mut g = AppGroup::new(name);
        g.icon = icon.into();
        g.color_hex = color.into();
        g
    }

    pub fn hide_element(&mut self, selector: &str, host: &str) {
        let list = self.hidden_elements.entry(host.to_string()).or_default();
        if !list.iter().any(|s| s == selector) {
            list.push(selector.to_string());
        }
    }

    /// Drops the host's entry once its last selector goes.
    pub fn show_element(&mut self, selector: &str, host: &str) {
        if let Some(list) = self.hidden_elements.get_mut(host) {
            list.retain(|s| s != selector);
            if list.is_empty() {
                self.hidden_elements.remove(host);
            }
        }
    }
}

lenient!(AppGroup, AppGroup::new(""), {
    req "name" => name, field "id" => id, field "icon" => icon, field "colorHex" => color_hex,
    field "dockApps" => dock_apps, field "blockedApps" => blocked_apps, field "allowSites" => allow_sites,
    field "blockSites" => block_sites, field "hiddenElements" => hidden_elements, field "jevEnabled" => jev_enabled,
    field "jevThreshold" => jev_threshold, field "tempoPreset" => tempo_preset, field "desktop" => desktop,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum VoiceProvider {
    #[default]
    System,
    Elevenlabs,
    Openai,
}

impl VoiceProvider {
    pub const ALL: [VoiceProvider; 3] = [VoiceProvider::System, VoiceProvider::Elevenlabs, VoiceProvider::Openai];
    pub fn label(self) -> &'static str {
        match self {
            VoiceProvider::System => "Built-in voices (free)",
            VoiceProvider::Elevenlabs => "ElevenLabs",
            VoiceProvider::Openai => "OpenAI",
        }
    }
    pub fn raw(self) -> &'static str {
        match self {
            VoiceProvider::System => "system",
            VoiceProvider::Elevenlabs => "elevenlabs",
            VoiceProvider::Openai => "openai",
        }
    }
    /// Keyring account holding this provider's API key.
    pub fn key_account(self) -> Option<&'static str> {
        match self {
            VoiceProvider::System => None,
            p => Some(p.raw()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    #[default]
    Todo,
    Done,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskItem {
    pub id: Id,
    pub title: String,
    pub notes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<Id>,
    /// 0 none, 1 low, 2 medium, 3 high
    pub priority: i32,
    #[serde(skip_serializing_if = "Option::is_none", with = "time::iso_opt")]
    pub due: Option<Date>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate_min: Option<i32>,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Id>,
    #[serde(with = "time::iso")]
    pub created_at: Date,
    #[serde(skip_serializing_if = "Option::is_none", with = "time::iso_opt")]
    pub completed_at: Option<Date>,
    pub listened_sec: i64,
}

impl TaskItem {
    pub fn new(title: &str, group_id: Option<Id>) -> Self {
        TaskItem {
            id: Id::new(),
            title: title.to_string(),
            notes: String::new(),
            group_id,
            priority: 0,
            due: None,
            estimate_min: None,
            status: TaskStatus::Todo,
            parent_id: None,
            created_at: time::now(),
            completed_at: None,
            listened_sec: 0,
        }
    }
    pub fn is_done(&self) -> bool {
        self.status == TaskStatus::Done
    }
}

lenient!(TaskItem, TaskItem::new("", None), {
    req "title" => title, field "id" => id, field "notes" => notes, opt "groupId" => group_id,
    field "priority" => priority, opt "due" => due, opt "estimateMin" => estimate_min, field "status" => status,
    opt "parentId" => parent_id, field "createdAt" => created_at, opt "completedAt" => completed_at,
    field "listenedSec" => listened_sec,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SessionKind {
    #[default]
    #[serde(rename = "focus")]
    Focus,
    #[serde(rename = "break")]
    Rest,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<Id>,
    #[serde(with = "time::iso")]
    pub start: Date,
    #[serde(with = "time::iso")]
    pub end: Date,
    /// Active seconds spent in the phase, excluding paused intervals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_seconds: Option<f64>,
    pub kind: SessionKind,
    pub interruptions: i32,
    pub completed: bool,
}

impl Session {
    pub fn new(task_id: Option<Id>, group_id: Option<Id>, start: Date, end: Date, kind: SessionKind) -> Self {
        Session {
            id: Id::new(),
            task_id,
            group_id,
            start,
            end,
            active_seconds: None,
            kind,
            interruptions: 0,
            completed: true,
        }
    }
    pub fn duration(&self) -> f64 {
        self.active_seconds.unwrap_or_else(|| time::since(self.end, self.start)).max(0.0)
    }
    pub fn minutes(&self) -> f64 {
        self.duration() / 60.0
    }
}

lenient!(Session, Session::new(None, None, time::distant_past(), time::distant_past(), SessionKind::Focus), {
    req "start" => start, req "end" => end, field "id" => id, opt "taskId" => task_id,
    opt "groupId" => group_id, opt "activeSeconds" => active_seconds, field "kind" => kind,
    field "interruptions" => interruptions, field "completed" => completed,
});

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockEvent {
    pub id: Id,
    #[serde(with = "time::iso")]
    pub time: Date,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<Id>,
    /// Hostname for sites, desktop id for apps.
    pub target: String,
    /// What to show for `target`: the app's name. None for sites.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// "rule", "jev" or "app"
    pub layer: String,
    pub overridden: bool,
}

impl BlockEvent {
    pub fn new(group_id: Option<Id>, target: &str, label: Option<String>, layer: &str, overridden: bool, time: Date) -> Self {
        BlockEvent { id: Id::new(), time, group_id, target: target.into(), label, layer: layer.into(), overridden }
    }
    pub fn display_name(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.target)
    }
}

lenient!(BlockEvent, BlockEvent::new(None, "", None, "rule", false, time::distant_past()), {
    req "target" => target, req "time" => time, field "id" => id, opt "groupId" => group_id,
    opt "label" => label, field "layer" => layer, field "overridden" => overridden,
});

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoPreset {
    pub name: String,
    pub focus_min: i32,
    pub short_break_min: i32,
    pub long_break_min: i32,
    pub cycles_before_long: i32,
    pub strict: bool,
}

impl TempoPreset {
    pub fn new(name: &str, focus: i32, short: i32, long: i32, strict: bool) -> Self {
        TempoPreset {
            name: name.into(),
            focus_min: focus,
            short_break_min: short,
            long_break_min: long,
            cycles_before_long: 4,
            strict,
        }
    }
    pub fn defaults() -> Vec<TempoPreset> {
        vec![
            TempoPreset::new("Classic", 25, 5, 15, false),
            TempoPreset::new("Deep work", 50, 10, 30, true),
            TempoPreset::new("Sprint", 15, 3, 10, false),
        ]
    }
}

lenient!(TempoPreset, TempoPreset::new("", 25, 5, 15, false), {
    req "name" => name, field "focusMin" => focus_min, field "shortBreakMin" => short_break_min,
    field "longBreakMin" => long_break_min, field "cyclesBeforeLong" => cycles_before_long, field "strict" => strict,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DockEdge {
    #[default]
    Left,
    Right,
    Bottom,
}

impl DockEdge {
    pub const ALL: [DockEdge; 3] = [DockEdge::Left, DockEdge::Right, DockEdge::Bottom];
    pub fn label(self) -> &'static str {
        match self {
            DockEdge::Left => "Left",
            DockEdge::Right => "Right",
            DockEdge::Bottom => "Bottom",
        }
    }
}

/// How Kairos Read's 3D scenes reach Claude.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SceneBackend {
    /// The Claude Agent SDK helper, on the Claude subscription Claude Code is logged in to.
    #[default]
    Subscription,
    /// The Messages API with the Anthropic key in Settings > API keys.
    ApiKey,
}

impl SceneBackend {
    pub const ALL: [SceneBackend; 2] = [SceneBackend::Subscription, SceneBackend::ApiKey];
    pub fn label(self) -> &'static str {
        match self {
            SceneBackend::Subscription => "Claude subscription",
            SceneBackend::ApiKey => "Anthropic API key",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BarAppearance {
    #[default]
    Dark,
    Glass,
}

impl BarAppearance {
    pub const ALL: [BarAppearance; 2] = [BarAppearance::Dark, BarAppearance::Glass];
    pub fn label(self) -> &'static str {
        match self {
            BarAppearance::Dark => "Dark",
            BarAppearance::Glass => "Glass",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub dock_edge: DockEdge,
    pub dock_visible: bool,
    pub icon_size: f64,
    pub switch_behavior: SwitchBehavior,
    pub presets: Vec<TempoPreset>,
    pub never_analyse: Vec<String>,
    /// When Jev is slow or offline: false lets the page through, true blocks it.
    pub jev_fail_closed: bool,
    pub max_overrides_per_session: i32,
    pub override_minutes: i32,
    pub break_overlay: bool,
    /// Ctrl+1 to Ctrl+9 switch groups. Off by default: browsers use them for tabs.
    pub group_hotkeys: bool,
    /// A `hotkeys::QUICK_ADD` id, or "off".
    pub quick_add_hotkey: String,
    /// A `hotkeys::TIMER` id, or "off".
    pub timer_hotkey: String,
    pub voice_provider: VoiceProvider,
    /// Voice id for the chosen cloud provider; empty means that provider's default.
    pub voice_id: String,
    /// Slide Kairos Bar off its edge while the pointer is away.
    pub auto_hide_bar: bool,
    /// Keep the Cinnamon panel hidden while Kairos Bar is on screen.
    pub replaces_system_dock: bool,
    pub notifies_when_due: bool,
    /// Downloads and Trash at the end of Kairos Bar.
    pub shows_dock_folders: bool,
    /// After a break runs out, start the next focus phase at once instead of waiting.
    pub starts_focus_after_break: bool,
    /// In Focus and Strict, hide a browser that Kairos Shield is not running in.
    pub block_unshielded_browsers: bool,
    /// While Kairos Bar is pinned open, keep windows from sitting under it.
    pub keeps_windows_clear: bool,
    /// Hide Kairos Bar while a full-screen window is in front, even with auto-hide off.
    pub hides_in_full_screen: bool,
    pub bar_appearance: BarAppearance,
    pub scene_backend: SceneBackend,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            dock_edge: DockEdge::Left,
            dock_visible: true,
            icon_size: 36.0,
            switch_behavior: SwitchBehavior::Hide,
            presets: TempoPreset::defaults(),
            never_analyse: Settings::default_never_analyse(),
            jev_fail_closed: false,
            max_overrides_per_session: 3,
            override_minutes: 5,
            break_overlay: true,
            group_hotkeys: false,
            quick_add_hotkey: "control-option-space".into(),
            timer_hotkey: "control-option-p".into(),
            voice_provider: VoiceProvider::System,
            voice_id: String::new(),
            auto_hide_bar: false,
            replaces_system_dock: false,
            notifies_when_due: true,
            shows_dock_folders: true,
            starts_focus_after_break: false,
            block_unshielded_browsers: true,
            keeps_windows_clear: true,
            hides_in_full_screen: true,
            bar_appearance: BarAppearance::Dark,
            scene_backend: SceneBackend::Subscription,
        }
    }
}

impl Settings {
    pub fn default_never_analyse() -> Vec<String> {
        [
            "mail.google.com", "outlook.live.com", "outlook.office.com", "proton.me", "icloud.com",
            "paypal.com", "chase.com", "bankofamerica.com", "wellsfargo.com", "hsbc.com", "barclays.co.uk",
            "monzo.com", "revolut.com", "nhs.uk", "mychart.com", "patient.info", "1password.com",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    /// A voice id belongs to one provider, so choosing another starts from its default voice.
    pub fn set_voice_provider(&mut self, provider: VoiceProvider) {
        if provider == self.voice_provider {
            return;
        }
        self.voice_provider = provider;
        self.voice_id.clear();
    }

    /// The panel stays hidden only while Kairos Bar is on screen to take its place.
    pub fn hides_system_dock(&self) -> bool {
        self.replaces_system_dock && self.dock_visible
    }

    /// "Use Kairos Bar instead of the panel". Turning it on also turns on auto-hide.
    pub fn set_replaces_system_dock(&mut self, on: bool) {
        if on && !self.replaces_system_dock {
            self.auto_hide_bar = true;
        }
        self.replaces_system_dock = on;
    }
}

lenient!(Settings, Settings::default(), {
    field "dockEdge" => dock_edge, field "dockVisible" => dock_visible, field "iconSize" => icon_size,
    field "switchBehavior" => switch_behavior, list "presets" => presets, field "neverAnalyse" => never_analyse,
    field "jevFailClosed" => jev_fail_closed, field "maxOverridesPerSession" => max_overrides_per_session,
    field "overrideMinutes" => override_minutes, field "breakOverlay" => break_overlay,
    field "groupHotkeys" => group_hotkeys, field "quickAddHotkey" => quick_add_hotkey, field "timerHotkey" => timer_hotkey,
    field "voiceProvider" => voice_provider, field "voiceId" => voice_id,
    field "autoHideBar" => auto_hide_bar, field "replacesSystemDock" => replaces_system_dock,
    field "blockUnshieldedBrowsers" => block_unshielded_browsers, field "startsFocusAfterBreak" => starts_focus_after_break,
    field "showsDockFolders" => shows_dock_folders, field "notifiesWhenDue" => notifies_when_due,
    field "keepsWindowsClear" => keeps_windows_clear, field "hidesInFullScreen" => hides_in_full_screen,
    field "barAppearance" => bar_appearance, field "sceneBackend" => scene_backend,
});

#[derive(Clone, Debug, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FocusState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_group_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_task_id: Option<Id>,
    pub mode: Mode,
    #[serde(skip_serializing_if = "Option::is_none", with = "time::iso_opt")]
    pub ends_at: Option<Date>,
    /// Group id -> the task that was current when that group was last left.
    pub task_by_group: BTreeMap<String, Id>,
    /// While a strict focus phase is locked: when it ends (or, paused, would end if resumed now).
    /// Saved so that quitting or a crash cannot end it, and pushed so Chrome keeps enforcing it.
    #[serde(skip_serializing_if = "Option::is_none", with = "time::iso_opt")]
    pub strict_until: Option<Date>,
}

lenient!(FocusState, FocusState::default(), {
    opt "activeGroupId" => active_group_id, opt "currentTaskId" => current_task_id, field "mode" => mode,
    opt "endsAt" => ends_at, opt "strictUntil" => strict_until, field "taskByGroup" => task_by_group,
});

/// Everything Kairos persists, in one JSON file.
#[derive(Clone, Debug, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppData {
    pub groups: Vec<AppGroup>,
    pub tasks: Vec<TaskItem>,
    pub sessions: Vec<Session>,
    pub block_events: Vec<BlockEvent>,
    pub settings: Settings,
    pub focus: FocusState,
    pub onboarded: bool,
    /// Extension clients ("shield", "read") that have connected at least once.
    pub seen_clients: Vec<String>,
    /// The owner hid the setup list on Today.
    pub setup_hidden: bool,
    /// The timer phase running at the last save, kept so a quit or crash still logs it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_session: Option<Session>,
    /// Kairos Read listening time while no task was current.
    pub unassigned_listened_sec: i64,
}

lenient!(AppData, AppData::default(), {
    list "groups" => groups, list "tasks" => tasks, list "sessions" => sessions,
    list "blockEvents" => block_events, field "settings" => settings, field "focus" => focus,
    field "onboarded" => onboarded, opt "openSession" => open_session,
    field "unassignedListenedSec" => unassigned_listened_sec,
    field "seenClients" => seen_clients, field "setupHidden" => setup_hidden,
});

impl AppData {
    /// Starter groups. `browser` is pinned where a browser belongs (site rules only reach a
    /// browser that can run Kairos Shield); `installed` keeps apps that are not on this machine
    /// off the bar.
    pub fn starter(browser: Option<&str>, installed: &dyn Fn(&str) -> bool) -> AppData {
        let pick = |ids: &[&str]| -> Vec<String> { ids.iter().filter(|id| installed(id)).map(|s| s.to_string()).collect() };
        let mut d = AppData::default();
        let mut work = AppGroup::with("Work", "briefcase.fill", "#4F7CFF");
        let mut work_apps = vec![];
        if let Some(b) = browser {
            work_apps.push(b.to_string());
        }
        work_apps.extend(pick(&["thunderbird.desktop", "org.mozilla.Thunderbird.desktop", "xed.desktop", "libreoffice-writer.desktop"]));
        work.dock_apps = work_apps;
        work.blocked_apps = vec!["discord.desktop".into(), "spotify.desktop".into(), "steam.desktop".into()];
        work.block_sites = ["youtube.com", "reddit.com", "x.com", "twitter.com", "instagram.com", "tiktok.com", "netflix.com"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut study = AppGroup::with("Study", "book.fill", "#2FA36B");
        let mut study_apps = vec![];
        if let Some(b) = browser {
            study_apps.push(b.to_string());
        }
        study_apps.extend(pick(&["xed.desktop", "xreader.desktop", "libreoffice-writer.desktop"]));
        study.dock_apps = study_apps;
        study.blocked_apps = vec!["discord.desktop".into(), "thunderbird.desktop".into(), "slack.desktop".into()];
        study.block_sites = ["x.com", "twitter.com", "instagram.com", "tiktok.com", "reddit.com"].iter().map(|s| s.to_string()).collect();
        let mut personal = AppGroup::with("Personal", "house.fill", "#E0703A");
        personal.dock_apps = pick(&[
            "org.gnome.Rhythmbox3.desktop",
            "pix.desktop",
            "io.github.celluloid_player.Celluloid.desktop",
            "hypnotix.desktop",
        ]);
        personal.jev_enabled = false;
        d.focus.active_group_id = Some(work.id);
        d.groups = vec![work, study, personal];
        d
    }

    pub fn group(&self, id: Option<Id>) -> Option<&AppGroup> {
        let id = id?;
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn active_group(&self) -> Option<&AppGroup> {
        self.group(self.focus.active_group_id).or_else(|| self.groups.first())
    }

    pub fn current_task(&self) -> Option<&TaskItem> {
        let id = self.focus.current_task_id?;
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn preset(&self, name: &str) -> TempoPreset {
        self.settings
            .presets
            .iter()
            .find(|p| p.name == name)
            .or_else(|| self.settings.presets.first())
            .cloned()
            .unwrap_or_else(|| TempoPreset::defaults().remove(0))
    }

    pub fn active_preset(&self) -> TempoPreset {
        let name = self.active_group().map(|g| g.tempo_preset.clone()).unwrap_or_else(|| "Classic".into());
        self.preset(&name)
    }
}
