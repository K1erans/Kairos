//! Owns all state. The main window, Kairos Bar, the tray icon, the app guard and the browser
//! bridge all read from and write to this one object, on the GTK main thread. Work that waits
//! on the network runs on threads and hands its result back through `run_on_main`.

use crate::platform::apps::{Apps, RunningApp};
use crate::platform::speech::SpeechPlayer;
use crate::platform::system::{self, keys};
use crate::platform::x11::X11;
use gtk::glib;
use kairos_core::appguard::{self, GuardApp, Reason};
use kairos_core::bridge::{BridgeEvent, BridgeServer, ClientId};
use kairos_core::browsers;
use kairos_core::claude::{Cancel, ClaudeApi, ClaudeSubscription};
use kairos_core::history::GroupSwitch;
use kairos_core::jev::{JevClient, JevError, JevReadiness, Subject};
use kairos_core::node::{self, SceneSetup};
use kairos_core::privacy;
use kairos_core::quickadd;
use kairos_core::reminders;
use kairos_core::rules::{self, OverrideLedger, Refusal, SiteList};
use kairos_core::scenes::SceneWriter;
use kairos_core::setup::{SetupChecklist, SetupFacts};
use kairos_core::store::{self, DataStore};
use kairos_core::tempo::{menu_title, StrictConfirmation, TempoEngine, TempoEvent, TempoPhase};
use kairos_core::time::{self, Date};
use kairos_core::visibility::{GroupVisibilityTracker, VisibilityApp};
use kairos_core::voice::{CloudVoice, VoiceClient, OPENAI_VOICES};
use kairos_core::*;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum NoticeKind {
    App(String),
    Rest,
}

/// What the block overlay is showing.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockNotice {
    pub id: u64,
    pub kind: NoticeKind,
    pub title: String,
    pub detail: String,
    /// A point on the screen showing the blocked app, so the overlay appears where the owner looks.
    pub at: Option<(f64, f64)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReadStatus {
    pub state: String,
    pub title: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Today,
    Tasks,
    Groups,
    Rules,
    Stats,
    Settings,
}

impl Page {
    pub const ALL: [Page; 6] = [Page::Today, Page::Tasks, Page::Groups, Page::Rules, Page::Stats, Page::Settings];
    pub fn title(self) -> &'static str {
        match self {
            Page::Today => "Today",
            Page::Tasks => "Tasks",
            Page::Groups => "Groups",
            Page::Rules => "Rules",
            Page::Stats => "Stats",
            Page::Settings => "Settings",
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Page::Today => "weather-clear-symbolic",
            Page::Tasks => "view-list-symbolic",
            Page::Groups => "view-grid-symbolic",
            Page::Rules => "action-unavailable-symbolic",
            Page::Stats => "org.gnome.SystemMonitor-symbolic",
            Page::Settings => "emblem-system-symbolic",
        }
    }
    pub fn key(self) -> &'static str {
        self.title()
    }
}

/// What changed, so each view refreshes only what it shows.
pub mod change {
    pub const DATA: u32 = 1;
    pub const TICK: u32 = 2;
    pub const NOTICE: u32 = 4;
    pub const CLIENTS: u32 = 8;
    pub const KEYS: u32 = 16;
    pub const RUNNING: u32 = 32;
    pub const NAV: u32 = 64;
    pub const ALL: u32 = 0xffff;
}

pub struct State {
    pub data: AppData,
    pub tempo: TempoEngine,
    pub now: Date,
    pub bridge_clients: Vec<String>,
    pub read_status: Option<ReadStatus>,
    pub suggestion: Option<TaskItem>,
    pub notice: Option<BlockNotice>,
    pub has_jev_key: bool,
    pub has_claude_key: bool,
    pub scene_setup: SceneSetup,
    pub voice_keys: HashSet<String>,
    pub cloud_voices: Vec<CloudVoice>,
    pub last_voice_error: Option<String>,
    pub last_jev_error: Option<String>,
    pub hotkey_failures: Vec<String>,
    pub running: Vec<RunningApp>,
    pub running_ids: HashSet<String>,
    /// Open apps' ids in launch order, without Kairos.
    pub open_apps: Vec<String>,
    /// Open apps currently hidden by Kairos for a group switch, kept off Kairos Bar until opened again.
    pub hidden_by_switch: HashSet<String>,
    pub page: Page,
    pub selected_group_id: Option<Id>,
    pub rename_group_id: Option<Id>,
    pub ledger: OverrideLedger,
}

impl State {
    pub fn mode(&self) -> Mode {
        self.data.focus.mode
    }
    pub fn active_group(&self) -> Option<&AppGroup> {
        self.data.active_group()
    }
    pub fn current_task(&self) -> Option<&TaskItem> {
        self.data.current_task()
    }
    pub fn remaining(&self) -> f64 {
        self.tempo.remaining(self.now)
    }
    pub fn overrides_left(&self) -> i32 {
        self.ledger.remaining(self.data.settings.max_overrides_per_session)
    }
    /// A strict focus phase ends only when its time runs out: Skip, Stop and Quit do nothing in it.
    pub fn strict_locked(&self) -> bool {
        self.tempo.phase() == TempoPhase::Focus && self.mode() == Mode::Strict
    }
    pub fn shield_connected(&self) -> bool {
        self.bridge_clients.iter().any(|c| c == "shield")
    }
    /// Blocking is on but no Kairos Shield is connected, so no site rule is enforced anywhere.
    pub fn site_blocking_off(&self) -> bool {
        self.mode().blocks() && !self.shield_connected()
    }
    pub fn cloud_voice_ready(&self) -> bool {
        self.data.settings.voice_provider.key_account().map(|a| self.voice_keys.contains(a)).unwrap_or(false)
    }
    pub fn jev_readiness(&self, group: Option<&AppGroup>) -> JevReadiness {
        JevReadiness::new(group.map(|g| g.jev_enabled).unwrap_or(false), self.has_jev_key, self.current_task().is_some(), self.last_jev_error.as_deref())
    }
    pub fn menu_title(&self) -> String {
        let name = self.current_task().map(|t| t.title.clone()).or_else(|| self.active_group().map(|g| g.name.clone())).unwrap_or_else(|| "Kairos".into());
        menu_title::make(&name, self.tempo.phase(), self.remaining(), self.tempo.awaiting_focus())
    }
    pub fn tasks_in(&self, group_id: Option<Id>, done: bool) -> Vec<TaskItem> {
        self.data.tasks.iter().filter(|t| t.group_id == group_id && t.is_done() == done && t.parent_id.is_none()).cloned().collect()
    }
    pub fn subtasks(&self, id: Id) -> Vec<TaskItem> {
        self.data.tasks.iter().filter(|t| t.parent_id == Some(id)).cloned().collect()
    }
    pub fn focus_minutes(&self, day: chrono::NaiveDate, group_id: Option<Id>) -> f64 {
        self.data
            .sessions
            .iter()
            .filter(|s| s.kind == SessionKind::Focus && time::local(s.start).date_naive() == day && (group_id.is_none() || s.group_id == group_id))
            .map(|s| s.minutes())
            .sum()
    }
    pub fn notice_can_override(&self) -> bool {
        matches!(self.notice.as_ref().map(|n| &n.kind), Some(NoticeKind::App(_))) && self.mode() == Mode::Focus && self.overrides_left() > 0
    }
    pub fn can_import(&self) -> bool {
        self.tempo.phase() == TempoPhase::Idle
    }
    pub fn rule_lock(&self, id: Id) -> Option<String> {
        if !self.strict_locked() || Some(id) != self.active_group().map(|g| g.id) {
            return None;
        }
        Some(rules::rule_lock::note(self.data.focus.strict_until))
    }
    pub fn is_app_blocked(&self, app: &str) -> bool {
        self.active_group().map(|g| g.blocked_apps.iter().any(|a| a == app)).unwrap_or(false)
    }
    pub fn setup_checklist(&self) -> SetupChecklist {
        let mut f = SetupFacts::default();
        f.seen_clients = self.data.seen_clients.iter().cloned().chain(self.bridge_clients.iter().cloned()).collect();
        f.has_jev_key = self.has_jev_key;
        f.has_claude_key = self.has_claude_key;
        f.scene_backend = self.data.settings.scene_backend;
        f.has_node = self.scene_setup.node.is_some();
        f.has_scene_helper = self.scene_setup.helper_installed;
        f.voice_provider = self.data.settings.voice_provider;
        f.has_voice_key = self.cloud_voice_ready();
        f.open_at_login = system::autostart::enabled();
        f.replaces_dock = self.data.settings.replaces_system_dock;
        SetupChecklist::new(&f)
    }
}

pub const SITE_BLOCKING_OFF_TEXT: &str = "Site blocking is off: Kairos Shield isn\u{2019}t connected, so your blocked sites open normally. Open Chrome (or another Chromium browser) with Kairos Shield loaded.";

/// The parts of the data that `data_changed` reacts to.
struct Before {
    mode: Mode,
    active_group: Option<Id>,
    switch_behavior: SwitchBehavior,
}

type Job = Box<dyn FnOnce(&Rc<Model>) + Send>;

/// Answers one bridge request; Send, so threads can answer when their work is done.
#[derive(Clone)]
pub struct Reply {
    bridge: BridgeServer,
    client: ClientId,
    id: Option<String>,
}

impl Reply {
    pub fn send(&self, mut payload: Value) {
        if let (Some(id), Some(obj)) = (&self.id, payload.as_object_mut()) {
            obj.insert("id".into(), json!(id));
        }
        self.bridge.send(&payload, self.client);
    }
}

#[derive(Default)]
pub struct Hooks {
    pub show_main_window: Option<Rc<dyn Fn()>>,
    pub notify: Option<Rc<dyn Fn(Option<&str>, &str, &str)>>,
    pub withdraw_notification: Option<Rc<dyn Fn(&str)>>,
    /// Asks before a strict focus phase; runs the closure once the owner agrees.
    pub confirm_strict: Option<Rc<dyn Fn(StrictConfirmation, Box<dyn FnOnce()>)>>,
}

pub struct Model {
    pub state: RefCell<State>,
    listeners: RefCell<Vec<Rc<dyn Fn(u32)>>>,
    store: DataStore,
    pub bridge: BridgeServer,
    jev: Arc<JevClient>,
    voices: Arc<VoiceClient>,
    speech: Rc<SpeechPlayer>,
    speech_generation: Cell<u64>,
    scene_tasks: RefCell<HashMap<String, Cancel>>,
    pub apps: Rc<Apps>,
    pub x: Option<Arc<X11>>,
    visibility: RefCell<GroupVisibilityTracker<String>>,
    hidden_before: RefCell<HashMap<String, bool>>,
    guards_apps: Cell<bool>,
    save_pending: RefCell<Option<glib::SourceId>>,
    last_save: Cell<Instant>,
    last_pushed: RefCell<String>,
    grant_timer: RefCell<Option<glib::SourceId>>,
    reminder_inputs: RefCell<Option<reminders::Inputs>>,
    /// Pending due-reminder timers by key. Each removes its own entry as it fires: removing a
    /// source that has already fired panics in glib.
    reminder_timers: RefCell<HashMap<u64, glib::SourceId>>,
    reminder_seq: Cell<u64>,
    reminder_debounce: RefCell<Option<glib::SourceId>>,
    reminder_ids: RefCell<Vec<String>>,
    notice_count: Cell<u64>,
    main: async_channel::Sender<Job>,
    pub hooks: RefCell<Hooks>,
}

impl Model {
    pub fn new(x: Option<Arc<X11>>) -> Rc<Model> {
        let store = DataStore::standard();
        let mut loaded = store.load(|| AppData::starter(browsers::preferred_chromium(&|id| Apps::is_installed(id)), &|id| Apps::is_installed(id)));
        let now = time::now();
        let resume = loaded.recover_after_launch(now);
        let preset = loaded.preset(&loaded.active_group().map(|g| g.tempo_preset.clone()).unwrap_or_else(|| "Classic".into()));
        let mut tempo = TempoEngine::new(preset);
        if let Some(r) = &resume {
            tempo.resume_strict(r);
        }
        let (tx, rx) = async_channel::unbounded::<Job>();
        let state = State {
            data: loaded,
            tempo,
            now,
            bridge_clients: vec![],
            read_status: None,
            suggestion: None,
            notice: None,
            has_jev_key: false,
            has_claude_key: false,
            scene_setup: SceneSetup::default(),
            voice_keys: HashSet::new(),
            cloud_voices: vec![],
            last_voice_error: None,
            last_jev_error: None,
            hotkey_failures: vec![],
            running: vec![],
            running_ids: HashSet::new(),
            open_apps: vec![],
            hidden_by_switch: HashSet::new(),
            page: Page::Today,
            selected_group_id: None,
            rename_group_id: None,
            ledger: OverrideLedger::new(),
        };
        let model = Rc::new(Model {
            state: RefCell::new(state),
            listeners: RefCell::new(vec![]),
            store,
            bridge: BridgeServer::new(kairos_core::bridge::socket_path()),
            jev: Arc::new(JevClient::default()),
            voices: Arc::new(VoiceClient::default()),
            speech: Rc::new(SpeechPlayer::default()),
            speech_generation: Cell::new(0),
            scene_tasks: RefCell::new(HashMap::new()),
            apps: Rc::new(Apps::new(x.clone())),
            x,
            visibility: RefCell::new(GroupVisibilityTracker::new()),
            hidden_before: RefCell::new(HashMap::new()),
            guards_apps: Cell::new(false),
            save_pending: RefCell::new(None),
            last_save: Cell::new(Instant::now()),
            last_pushed: RefCell::new(String::new()),
            grant_timer: RefCell::new(None),
            reminder_inputs: RefCell::new(None),
            reminder_timers: RefCell::new(HashMap::new()),
            reminder_seq: Cell::new(0),
            reminder_debounce: RefCell::new(None),
            reminder_ids: RefCell::new(vec![]),
            notice_count: Cell::new(0),
            main: tx,
            hooks: RefCell::new(Hooks::default()),
        });
        let weak = Rc::downgrade(&model);
        glib::spawn_future_local(async move {
            while let Ok(job) = rx.recv().await {
                let Some(m) = weak.upgrade() else { break };
                job(&m);
            }
        });
        model.refresh_keys();
        model
    }

    /// Runs `job` on the main thread with the model. Callable from any thread.
    pub fn sender(&self) -> async_channel::Sender<Job> {
        self.main.clone()
    }

    pub fn subscribe(&self, f: impl Fn(u32) + 'static) {
        self.listeners.borrow_mut().push(Rc::new(f));
    }

    pub fn emit(&self, what: u32) {
        let list: Vec<Rc<dyn Fn(u32)>> = self.listeners.borrow().clone();
        for l in list {
            l(what);
        }
    }

    pub fn s(&self) -> std::cell::Ref<'_, State> {
        self.state.borrow()
    }

    /// Changes state other than the data, then tells the views.
    pub fn set(&self, what: u32, f: impl FnOnce(&mut State)) {
        f(&mut self.state.borrow_mut());
        self.emit(what);
    }

    /// Changes the data: saves soon, tells the extensions and the views, and reacts to a change
    /// of mode or group.
    pub fn edit(self: &Rc<Self>, f: impl FnOnce(&mut AppData)) {
        let before = {
            let s = self.state.borrow();
            Before { mode: s.data.focus.mode, active_group: s.data.active_group().map(|g| g.id), switch_behavior: s.data.active_switch_behavior() }
        };
        f(&mut self.state.borrow_mut().data);
        self.data_changed(before);
    }

    // MARK: Lifecycle

    /// Why the data file couldn't be read, while Kairos refuses to save over it.
    pub fn store_problem(&self) -> Option<String> {
        self.store.problem()
    }

    pub fn start(self: &Rc<Self>) {
        self.guards_apps.set(!store::is_scratch() && self.x.is_some());
        let tx = self.main.clone();
        let started = self.bridge.start(Arc::new(move |event| {
            let job: Job = match event {
                BridgeEvent::Message(client, message) => Box::new(move |m: &Rc<Model>| m.handle(message, client)),
                BridgeEvent::ClientsChanged => Box::new(|m: &Rc<Model>| {
                    let names = m.bridge.client_names();
                    m.set(change::CLIENTS, |s| {
                        if !names.iter().any(|n| n == "read") {
                            s.read_status = None;
                        }
                        s.bridge_clients = names;
                    });
                }),
            };
            let _ = tx.send_blocking(job);
        }));
        if let Err(e) = started {
            eprintln!("Kairos: bridge failed to start: {e:?}");
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let Some(m) = weak.upgrade() else { return glib::ControlFlow::Break };
            m.tick();
            glib::ControlFlow::Continue
        });
        self.refresh_running();
        self.save_now();
        if self.s().mode().blocks() {
            self.sweep_soon();
        }
        self.schedule_reminders_soon();
    }

    pub fn shutdown(self: &Rc<Self>) {
        self.restore_visibility();
        self.bridge.stop();
        self.save_now();
    }

    fn data_changed(self: &Rc<Self>, before: Before) {
        let (mode, group, behavior) = {
            let s = self.s();
            (s.data.focus.mode, s.data.active_group().map(|g| g.id), s.data.active_switch_behavior())
        };
        if before.mode != mode {
            self.mode_changed(before.mode, mode);
        }
        if self.guards_apps.get() {
            if before.active_group != group {
                self.apply_visibility(before.active_group);
                self.sweep_soon();
            } else if before.switch_behavior != SwitchBehavior::Leave && behavior == SwitchBehavior::Leave {
                self.restore_visibility();
                self.sweep_soon();
            }
        }
        self.save_soon();
        self.push_state(false);
        self.schedule_reminders_soon();
        self.emit(change::DATA);
    }

    fn save_soon(self: &Rc<Self>) {
        if let Some(id) = self.save_pending.borrow_mut().take() {
            id.remove();
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_millis(500), move || {
            if let Some(m) = weak.upgrade() {
                m.save_pending.borrow_mut().take();
                m.save_now();
            }
        });
        *self.save_pending.borrow_mut() = Some(id);
    }

    /// Saves with the running phase as `openSession`, so a quit or crash can still log it.
    pub fn save_now(&self) {
        let t = time::now();
        let mut snapshot = {
            let s = self.s();
            let mut d = s.data.clone();
            d.open_session = s.tempo.current(t).and_then(|e| Session::from_event(&e, s.data.focus.current_task_id, s.data.focus.active_group_id));
            d
        };
        snapshot.trim_history(t);
        self.last_save.set(Instant::now());
        if let Err(e) = self.store.save(&snapshot) {
            eprintln!("Kairos: save failed: {e}");
        }
    }

    pub fn export_json(&self) -> Vec<u8> {
        store::encode(&self.s().data)
    }

    /// Replaces everything with an exported file, after saving and backing up the current file.
    pub fn import_data(self: &Rc<Self>, file: AppData) -> Result<Option<std::path::PathBuf>, String> {
        if !self.s().can_import() {
            return Err(kairos_core::import::ImportProblem::TimerRunning.to_string());
        }
        self.save_now();
        let backup = self.store.backup(time::now()).map_err(|e| e.to_string())?;
        self.edit(|d| *d = d.importing(file, time::now()));
        let preset = { let s = self.s(); s.data.active_preset() };
        self.state.borrow_mut().tempo.use_preset(preset);
        Ok(backup)
    }

    // MARK: Keys

    pub fn refresh_keys(self: &Rc<Self>) {
        let has = |a: &str| keys::get(a).map(|k| !k.is_empty()).unwrap_or(false);
        let jev = has("typesafe");
        let claude = has("anthropic");
        let voice: HashSet<String> = ["elevenlabs", "openai"].iter().filter(|a| has(a)).map(|s| s.to_string()).collect();
        self.set(change::KEYS, |s| {
            s.has_jev_key = jev;
            s.has_claude_key = claude;
            s.voice_keys = voice;
        });
        self.refresh_scene_setup();
    }

    /// Looks for `node` off the main thread: asking the login shell can take a moment.
    pub fn refresh_scene_setup(&self) {
        let tx = self.main.clone();
        std::thread::spawn(move || {
            let setup = SceneSetup::new(node::locate(), node::helper_url(system::resources().as_deref()));
            let _ = tx.send_blocking(Box::new(move |m: &Rc<Model>| {
                if m.s().scene_setup != setup {
                    m.set(change::KEYS, |s| s.scene_setup = setup);
                }
            }));
        });
    }

    pub fn load_cloud_voices(&self) {
        let provider = self.s().data.settings.voice_provider;
        match provider {
            VoiceProvider::System => self.set(change::KEYS, |s| s.cloud_voices = vec![]),
            VoiceProvider::Openai => {
                let list = OPENAI_VOICES.iter().map(|v| CloudVoice { id: v.to_string(), name: capitalize(v) }).collect();
                self.set(change::KEYS, |s| s.cloud_voices = list);
            }
            VoiceProvider::Elevenlabs => {
                let Some(key) = keys::get("elevenlabs") else {
                    self.set(change::KEYS, |s| s.cloud_voices = vec![]);
                    return;
                };
                let voices = self.voices.clone();
                let tx = self.main.clone();
                std::thread::spawn(move || {
                    let result = voices.elevenlabs_voices(&key);
                    let _ = tx.send_blocking(Box::new(move |m: &Rc<Model>| match result {
                        Ok(list) => m.set(change::KEYS, |s| {
                            s.cloud_voices = list;
                            s.last_voice_error = None;
                        }),
                        Err(e) => m.set(change::KEYS, |s| s.last_voice_error = Some(e.to_string())),
                    }));
                });
            }
        }
    }

    pub fn set_key(self: &Rc<Self>, value: &str, account: &str) {
        keys::set(value.trim(), account);
        self.refresh_keys();
        self.push_state(false);
    }

    // MARK: Tasks

    pub fn add_task(self: &Rc<Self>, line: &str) -> Option<TaskItem> {
        let names: Vec<String> = self.s().data.groups.iter().map(|g| g.name.clone()).collect();
        let parsed = quickadd::parse(line, &names, time::now());
        if parsed.title.is_empty() {
            return None;
        }
        let task = {
            let s = self.s();
            let group = parsed.group_name.as_ref().and_then(|n| s.data.groups.iter().find(|g| g.name.to_lowercase() == n.to_lowercase()));
            let mut t = TaskItem::new(&parsed.title, group.map(|g| g.id).or(s.data.focus.active_group_id));
            t.due = parsed.due;
            t.estimate_min = parsed.estimate_min;
            t.priority = parsed.priority;
            t
        };
        let copy = task.clone();
        self.edit(|d| d.tasks.push(task));
        Some(copy)
    }

    pub fn add_subtask(self: &Rc<Self>, title: &str, parent: &TaskItem) {
        let t = title.trim();
        if t.is_empty() {
            return;
        }
        let mut task = TaskItem::new(t, parent.group_id);
        task.parent_id = Some(parent.id);
        self.edit(|d| d.tasks.push(task));
    }

    pub fn update_task(self: &Rc<Self>, id: Id, f: impl FnOnce(&mut TaskItem)) {
        self.edit(|d| {
            if let Some(t) = d.tasks.iter_mut().find(|t| t.id == id) {
                f(t);
            }
        });
    }

    pub fn set_done(self: &Rc<Self>, id: Id, done: bool) {
        let now = time::now();
        let was_current = self.s().data.focus.current_task_id == Some(id);
        self.edit(|d| {
            if let Some(t) = d.tasks.iter_mut().find(|t| t.id == id) {
                t.status = if done { TaskStatus::Done } else { TaskStatus::Todo };
                t.completed_at = if done { Some(now) } else { None };
            }
            if done && was_current {
                d.focus.current_task_id = None;
            }
        });
        if done && was_current {
            let next = {
                let s = self.s();
                let available = (s.tempo.phase() == TempoPhase::Focus).then(|| (s.remaining() / 60.0) as i32);
                quickadd::suggest(&s.data.tasks, s.data.focus.active_group_id, Some(id), available)
            };
            self.set(change::DATA, |s| s.suggestion = next);
        }
    }

    pub fn delete_task(self: &Rc<Self>, id: Id) {
        self.edit(|d| {
            d.tasks.retain(|t| t.id != id && t.parent_id != Some(id));
            if d.focus.current_task_id == Some(id) {
                d.focus.current_task_id = None;
            }
        });
    }

    /// Makes a task current and turns focus on. The timer is started separately.
    pub fn make_current(self: &Rc<Self>, task: &TaskItem) {
        self.state.borrow_mut().suggestion = None;
        if let Some(g) = task.group_id {
            if Some(g) != self.s().data.focus.active_group_id {
                self.switch_group(g);
            }
        }
        let id = task.id;
        self.edit(|d| d.focus.current_task_id = Some(id));
        if self.s().mode() == Mode::Off {
            self.set_mode(Mode::Focus);
        }
    }

    /// "Start" on a task: make it current and, when no timer runs, start a focus session on it.
    pub fn start_task(self: &Rc<Self>, task: &TaskItem) {
        self.state.borrow_mut().suggestion = None;
        if let Some(g) = task.group_id {
            if Some(g) != self.s().data.focus.active_group_id {
                self.switch_group(g);
            }
        }
        let id = task.id;
        self.edit(|d| d.focus.current_task_id = Some(id));
        if self.s().tempo.phase() == TempoPhase::Idle {
            self.request_start_focus();
        }
    }

    // MARK: Groups and mode

    pub fn switch_group(self: &Rc<Self>, id: Id) {
        let (next, desktop) = {
            let s = self.s();
            if Some(id) == s.data.focus.active_group_id {
                return;
            }
            let Some(g) = s.data.group(Some(id)) else { return };
            (s.data.focus_after_switch(id), g.desktop)
        };
        self.edit(|d| d.focus = next);
        let preset = { let s = self.s(); s.data.active_preset() };
        self.state.borrow_mut().tempo.use_preset(preset);
        if desktop > 0 {
            if let Some(x) = &self.x {
                x.switch_desktop(desktop as u32);
            }
        }
    }

    pub fn switch_group_index(self: &Rc<Self>, index: usize) {
        let id = self.s().data.groups.get(index).map(|g| g.id);
        if let Some(id) = id {
            self.switch_group(id);
        }
    }

    /// The Mode picker on Today and the tray's Mode menu, which lock while a timer runs (the
    /// timer owns the mode then). A press from a stale view, such as a tray menu opened before
    /// the timer started, is refused here too, so it cannot take a strict phase out of Strict.
    pub fn set_mode(self: &Rc<Self>, mode: Mode) {
        if mode == self.s().mode() || self.s().tempo.phase() != TempoPhase::Idle {
            return;
        }
        self.edit(|d| d.focus.mode = mode);
    }

    /// Runs on every change of mode, whoever made it: the menu, the timer or launch.
    fn mode_changed(self: &Rc<Self>, old: Mode, new: Mode) {
        // Entering Strict also clears grants, so an "Allow 5 min" made in Focus cannot carry over.
        let mut s = self.state.borrow_mut();
        if (new.blocks() && !old.blocks()) || (new == Mode::Strict && old != Mode::Strict) {
            s.ledger.reset();
        }
        let clear_notice = !new.blocks() && matches!(s.notice.as_ref().map(|n| &n.kind), Some(NoticeKind::App(_)));
        if clear_notice {
            s.notice = None;
        }
        drop(s);
        if clear_notice {
            self.emit(change::NOTICE);
        }
        if new.blocks() && !old.blocks() {
            self.sweep_soon();
        }
    }

    pub fn add_group(self: &Rc<Self>, name: &str) -> Id {
        let group = {
            let s = self.s();
            let colors = crate::ui::icons::COLORS;
            let color = colors[s.data.groups.len() % colors.len()].0;
            let icon = crate::ui::icons::GROUP_ICONS
                .iter()
                .map(|i| i.0)
                .find(|i| !s.data.groups.iter().any(|g| g.icon == *i))
                .unwrap_or("square.grid.2x2");
            AppGroup::with(name, icon, color)
        };
        let id = group.id;
        self.edit(|d| d.groups.push(group));
        id
    }

    pub fn rename_group(self: &Rc<Self>, id: Id, name: &str) {
        let n = name.trim().to_string();
        if n.is_empty() {
            return;
        }
        self.edit(|d| {
            if let Some(g) = d.groups.iter_mut().find(|g| g.id == id) {
                g.name = n;
            }
        });
    }

    pub fn delete_group(self: &Rc<Self>, id: Id) {
        if self.s().data.groups.len() < 2 {
            return;
        }
        self.edit(|d| {
            d.groups.retain(|g| g.id != id);
            for t in d.tasks.iter_mut().filter(|t| t.group_id == Some(id)) {
                t.group_id = None;
            }
            if d.focus.active_group_id == Some(id) {
                d.focus.active_group_id = d.groups.first().map(|g| g.id);
            }
        });
        let active = self.s().data.focus.active_group_id;
        if self.s().selected_group_id == Some(id) {
            self.set(change::NAV, |s| s.selected_group_id = active);
        }
    }

    /// Edits one group; edits that would loosen the active group's rules during a locked strict
    /// phase are refused (RuleLock). Returns whether the group changed.
    pub fn update_group(self: &Rc<Self>, id: Id, f: impl FnOnce(&mut AppGroup)) -> bool {
        let locked = self.s().strict_locked();
        let mut changed = false;
        self.edit(|d| changed = d.update_group(id, locked, f));
        if !changed {
            // A refused edit: views redraw from the unchanged data.
            self.emit(change::DATA);
        }
        changed
    }

    /// Sets one of a group's app lists, such as its Kairos Bar apps. Returns whether the list now
    /// reads `next`: false when the strict lock refused it.
    pub fn set_group_apps(self: &Rc<Self>, id: Id, next: Vec<String>, list: fn(&mut AppGroup) -> &mut Vec<String>) -> bool {
        let want = next.clone();
        self.update_group(id, move |g| *list(g) = next);
        let Some(mut g) = self.s().data.group(Some(id)).cloned() else { return false };
        *list(&mut g) == want
    }

    /// Edits Settings; a change that would loosen blocking during a locked strict phase is
    /// refused (RuleLock), and the views redraw from the unchanged data.
    pub fn update_settings(self: &Rc<Self>, f: impl FnOnce(&mut Settings)) {
        let locked = self.s().strict_locked();
        let mut changed = false;
        self.edit(|d| changed = d.update_settings(locked, f));
        if !changed {
            self.emit(change::DATA);
        }
    }

    /// Pins an app to a group's Kairos Bar (the active group's when `group` is None). An app in
    /// the active group's bar is trusted by Jev, so during a locked strict phase this is refused.
    pub fn add_dock_app(self: &Rc<Self>, app: &str, group: Option<Id>) -> bool {
        let Some(target) = group.or(self.s().data.focus.active_group_id) else { return false };
        let app = app.to_string();
        self.update_group(target, |g| {
            if !g.dock_apps.contains(&app) {
                g.dock_apps.push(app);
            }
        })
    }

    /// "Block in <group>" from Kairos Bar. A newly blocked app on screen is hidden at once.
    pub fn set_app_blocked(self: &Rc<Self>, app: &str, blocked: bool) {
        let locked = self.s().strict_locked();
        let mut changed = false;
        self.edit(|d| changed = d.set_app_blocked(app, blocked, locked));
        if changed && blocked {
            self.sweep_soon();
        }
    }

    pub fn remove_dock_app(self: &Rc<Self>, app: &str) {
        let Some(active) = self.s().data.focus.active_group_id else { return };
        self.update_group(active, |g| g.dock_apps.retain(|a| a != app));
    }

    pub fn move_dock_app_by(self: &Rc<Self>, app: &str, offset: i64, group: Option<Id>) {
        let Some(target) = group.or(self.s().data.focus.active_group_id) else { return };
        let app = app.to_string();
        self.update_group(target, |g| g.dock_apps = dock::moving_by(&g.dock_apps, &app, offset));
    }

    pub fn move_dock_app_to(self: &Rc<Self>, app: &str, index: usize) {
        let moved = {
            let s = self.s();
            let Some(g) = s.active_group() else { return };
            let moved = dock::moving_to(&g.dock_apps, &app.to_string(), index);
            if moved == g.dock_apps {
                return;
            }
            moved
        };
        let Some(active) = self.s().data.focus.active_group_id else { return };
        self.update_group(active, |g| g.dock_apps = moved);
    }

    // MARK: Tempo

    fn preset(&self) -> TempoPreset {
        self.s().data.active_preset()
    }

    /// Strict chosen by hand before the timer starts holds until the timer stops.
    pub fn start_focus(self: &Rc<Self>) {
        let preset = self.preset();
        let events = {
            let mut s = self.state.borrow_mut();
            let strict = s.mode() == Mode::Strict;
            s.tempo.use_preset(preset);
            s.tempo.start(TempoPhase::Focus, strict, time::now())
        };
        self.apply(events);
    }

    /// Start from a button or hotkey: a strict focus phase starts only once the owner agrees.
    pub fn request_start_focus(self: &Rc<Self>) {
        let preset = self.preset();
        if self.s().tempo.phase() != TempoPhase::Idle {
            return;
        }
        if self.s().mode() == Mode::Strict || preset.strict {
            let weak = Rc::downgrade(self);
            self.agree_to_strict(preset.focus_min, Box::new(move || {
                if let Some(m) = weak.upgrade() {
                    if m.s().tempo.phase() == TempoPhase::Idle {
                        m.start_focus();
                    }
                }
            }));
            return;
        }
        self.start_focus();
    }

    /// Skip from a break: skipping into a strict focus phase starts a lock, so it asks too.
    pub fn request_skip(self: &Rc<Self>) {
        let (asks, minutes) = {
            let s = self.s();
            (s.tempo.phase().is_break() && s.tempo.next_focus_is_strict(), s.data.active_preset().focus_min)
        };
        if asks {
            let weak = Rc::downgrade(self);
            self.agree_to_strict(minutes, Box::new(move || {
                if let Some(m) = weak.upgrade() {
                    m.skip_phase();
                }
            }));
            return;
        }
        self.skip_phase();
    }

    fn agree_to_strict(&self, minutes: i32, then: Box<dyn FnOnce()>) {
        let confirm = self.hooks.borrow().confirm_strict.clone();
        match confirm {
            Some(c) => c(StrictConfirmation::new(minutes, time::add(time::now(), minutes as f64 * 60.0)), then),
            None => then(),
        }
    }

    pub fn toggle_timer(self: &Rc<Self>) {
        let t = time::now();
        let (idle, paused) = {
            let s = self.s();
            (s.tempo.phase() == TempoPhase::Idle, s.tempo.is_paused())
        };
        if idle {
            self.request_start_focus();
        } else if paused {
            self.state.borrow_mut().tempo.resume(t);
            self.sync_focus_from_tempo();
        } else {
            self.state.borrow_mut().tempo.pause(t);
            self.sync_focus_from_tempo();
        }
    }

    pub fn skip_phase(self: &Rc<Self>) {
        if self.s().strict_locked() {
            return;
        }
        let events = self.state.borrow_mut().tempo.skip(time::now());
        self.apply(events);
    }

    pub fn stop_timer(self: &Rc<Self>) {
        if self.s().strict_locked() {
            return;
        }
        let events = self.state.borrow_mut().tempo.stop(time::now());
        self.apply(events);
    }

    pub fn add_five_minutes(self: &Rc<Self>) {
        self.state.borrow_mut().tempo.extend(5, time::now());
        self.sync_focus_from_tempo();
    }

    pub fn end_break_wait(self: &Rc<Self>) {
        self.state.borrow_mut().tempo.end_wait();
        self.emit(change::DATA);
    }

    fn tick(self: &Rc<Self>) {
        let now = time::now();
        let starts = self.s().data.settings.starts_focus_after_break;
        let events = {
            let mut s = self.state.borrow_mut();
            s.now = now;
            s.tempo.tick(now, starts)
        };
        if !events.is_empty() {
            self.apply(events);
        }
        // A paused strict phase would end later the longer it waits, so its saved end moves on.
        let resync = {
            let s = self.s();
            s.tempo.is_paused() && s.strict_locked() && s.data.focus.strict_until.map(|u| time::since(time::add(now, s.remaining()), u) > 30.0).unwrap_or(false)
        };
        if resync {
            self.sync_focus_from_tempo();
        }
        // Keeps `openSession` fresh, so a crash loses at most a minute of the running phase.
        if self.s().tempo.phase() != TempoPhase::Idle && self.last_save.get().elapsed().as_secs() >= 60 {
            self.save_now();
        }
        self.emit(change::TICK);
    }

    fn apply(self: &Rc<Self>, events: Vec<TempoEvent>) {
        for event in events {
            match &event {
                TempoEvent::Finished { phase, duration, completed, .. } => {
                    if *duration < kairos_core::history::MINIMUM_LENGTH {
                        continue;
                    }
                    let now = time::now();
                    self.edit(|d| d.log(&event, now));
                    let (task, awaiting) = {
                        let s = self.s();
                        (s.current_task().map(|t| t.title.clone()), s.tempo.awaiting_focus())
                    };
                    if *phase == TempoPhase::Focus && *completed {
                        let body = task.map(|t| format!("Done with \u{201C}{t}\u{201D}? Time for a break.")).unwrap_or_else(|| "Time for a break.".into());
                        self.notify(None, "Focus session done", &body);
                    } else if phase.is_break() && *completed {
                        let body = if awaiting {
                            task.map(|t| format!("Start \u{201C}{t}\u{201D} when you\u{2019}re ready.")).unwrap_or_else(|| "Start the next focus session when you\u{2019}re ready.".into())
                        } else {
                            task.map(|t| format!("Back to \u{201C}{t}\u{201D}.")).unwrap_or_else(|| "Ready for the next session.".into())
                        };
                        self.notify(None, "Break over", &body);
                    }
                }
                TempoEvent::Started(phase) => {
                    if *phase == TempoPhase::Focus {
                        let mut s = self.state.borrow_mut();
                        s.ledger.reset();
                        if s.notice.as_ref().map(|n| n.kind == NoticeKind::Rest).unwrap_or(false) {
                            s.notice = None;
                        }
                        drop(s);
                        self.emit(change::NOTICE);
                    } else if self.s().data.settings.break_overlay {
                        self.show_notice(NoticeKind::Rest, phase.label().into(), "Step away from the screen.".into(), None);
                    }
                }
                TempoEvent::Stopped | TempoEvent::AwaitingFocus => {
                    self.set(change::NOTICE, |s| s.notice = None);
                    self.edit(|d| d.focus.mode = Mode::Off);
                }
            }
        }
        self.sync_focus_from_tempo();
    }

    /// Mirrors the timer into FocusState, which is what the extensions and the guard follow.
    fn sync_focus_from_tempo(self: &Rc<Self>) {
        let (focus, changed) = {
            let s = self.s();
            let mut focus = s.data.focus.clone();
            if s.tempo.phase() != TempoPhase::Idle {
                focus.mode = s.tempo.mode();
            }
            focus.ends_at = if s.tempo.is_running() { s.tempo.ends_at() } else { None };
            focus.strict_until = if s.tempo.phase() == TempoPhase::Focus && focus.mode == Mode::Strict {
                if s.tempo.is_paused() {
                    Some(time::add(time::now(), s.tempo.remaining(time::now())))
                } else {
                    s.tempo.ends_at()
                }
            } else {
                None
            };
            let changed = focus != s.data.focus;
            (focus, changed)
        };
        if changed {
            self.edit(|d| d.focus = focus);
        } else {
            self.emit(change::DATA);
        }
    }

    fn notify(&self, id: Option<&str>, title: &str, body: &str) {
        if store::is_scratch() {
            return;
        }
        let hook = self.hooks.borrow().notify.clone();
        if let Some(n) = hook {
            n(id, title, body);
        }
    }

    // MARK: Reminders

    fn schedule_reminders_soon(self: &Rc<Self>) {
        if store::is_scratch() {
            return;
        }
        let inputs = reminders::Inputs::new(&self.s().data);
        if self.reminder_inputs.borrow().as_ref() == Some(&inputs) {
            return;
        }
        if let Some(id) = self.reminder_debounce.borrow_mut().take() {
            id.remove();
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_secs(1), move || {
            if let Some(m) = weak.upgrade() {
                m.reminder_debounce.borrow_mut().take();
                m.schedule_reminders();
            }
        });
        *self.reminder_debounce.borrow_mut() = Some(id);
    }

    /// Replaces the pending due reminders with the current plan, the soonest 64. Kairos fires
    /// them itself while it runs.
    pub fn schedule_reminders(self: &Rc<Self>) {
        let inputs = reminders::Inputs::new(&self.s().data);
        let plan = reminders::plan(&inputs, time::now());
        *self.reminder_inputs.borrow_mut() = Some(inputs);
        let pending: Vec<glib::SourceId> = self.reminder_timers.borrow_mut().drain().map(|(_, id)| id).collect();
        for id in pending {
            id.remove();
        }
        let withdraw = self.hooks.borrow().withdraw_notification.clone();
        let planned: HashSet<String> = plan.iter().map(|r| r.id.clone()).collect();
        if let Some(w) = withdraw {
            for old in self.reminder_ids.borrow().iter().filter(|i| !planned.contains(*i)) {
                w(old);
            }
        }
        *self.reminder_ids.borrow_mut() = planned.into_iter().collect();
        // glib counts a timeout in u32 milliseconds, so a wait over about 49 days would wrap and
        // fire early. Reminders further off than a day get one timer that plans again then.
        const LONGEST_WAIT: f64 = 24.0 * 3600.0;
        let mut replan_armed = false;
        for r in plan {
            let wait = time::since(r.fire_at, time::now()).max(0.0);
            if wait > LONGEST_WAIT {
                if !replan_armed {
                    replan_armed = true;
                    self.add_reminder_timer(LONGEST_WAIT, |m| m.schedule_reminders());
                }
                continue;
            }
            self.add_reminder_timer(wait, move |m| m.notify(Some(&r.id), &r.title, &r.body));
        }
    }

    /// A one-shot reminder timer that drops its own id from `reminder_timers` before it runs.
    fn add_reminder_timer(self: &Rc<Self>, wait: f64, f: impl FnOnce(&Rc<Model>) + 'static) {
        let key = self.reminder_seq.get() + 1;
        self.reminder_seq.set(key);
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_secs_f64(wait), move || {
            if let Some(m) = weak.upgrade() {
                m.reminder_timers.borrow_mut().remove(&key);
                f(&m);
            }
        });
        self.reminder_timers.borrow_mut().insert(key, id);
    }

    // MARK: Running apps and group visibility

    pub fn refresh_running(self: &Rc<Self>) {
        let list = self.apps.running();
        let ids: HashSet<String> = list.iter().map(|a| a.id.clone()).collect();
        let mut open: Vec<String> = vec![];
        for a in &list {
            if !open.contains(&a.id) {
                open.push(a.id.clone());
            }
        }
        // Hide, unhide and quit notices for the group visibility tracker.
        let now: HashMap<String, bool> = list.iter().map(|a| (a.key(), a.hidden)).collect();
        let before = std::mem::replace(&mut *self.hidden_before.borrow_mut(), now.clone());
        {
            let mut tracker = self.visibility.borrow_mut();
            for (key, was) in &before {
                match now.get(key) {
                    None => tracker.did_terminate(key),
                    Some(hidden) if hidden != was => {
                        if *hidden {
                            tracker.did_hide(key, &mut |k| self.unhide_key(&list, k));
                        } else {
                            tracker.did_unhide(key, false);
                        }
                    }
                    _ => {}
                }
            }
        }
        let hidden_by_switch = self.hidden_by_switch_ids(&list);
        let changed = {
            let s = self.s();
            s.running != list || s.open_apps != open || s.hidden_by_switch != hidden_by_switch
        };
        if changed {
            self.set(change::RUNNING, |s| {
                s.running = list;
                s.running_ids = ids;
                s.open_apps = open;
                s.hidden_by_switch = hidden_by_switch;
            });
        }
    }

    fn hidden_by_switch_ids(&self, list: &[RunningApp]) -> HashSet<String> {
        let tracker = self.visibility.borrow();
        let visible: HashSet<&str> = list.iter().filter(|a| !tracker.is_hidden_by_switch(&a.key())).map(|a| a.id.as_str()).collect();
        list.iter().filter(|a| !visible.contains(a.id.as_str())).map(|a| a.id.clone()).collect()
    }

    fn hide_key(&self, list: &[RunningApp], key: &str) -> bool {
        list.iter().find(|a| a.key() == key).map(|a| self.apps.hide(a)).unwrap_or(false)
    }

    fn unhide_key(&self, list: &[RunningApp], key: &str) -> bool {
        list.iter().find(|a| a.key() == key).map(|a| self.apps.unhide(a)).unwrap_or(false)
    }

    fn apply_visibility(self: &Rc<Self>, from_group: Option<Id>) {
        let list = self.apps.running();
        let policy = {
            let s = self.s();
            let active_id = s.active_group().map(|g| g.id);
            let other_group_apps: HashSet<String> = s
                .data
                .groups
                .iter()
                .filter(|g| Some(g.id) != active_id)
                .flat_map(|g| g.dock_apps.iter().cloned())
                .collect();
            GroupSwitch::with_other_groups(s.active_group(), s.data.active_switch_behavior(), other_group_apps)
        };
        let entries: Vec<VisibilityApp<String>> = list.iter().map(|a| VisibilityApp { app: a.key(), app_id: Some(a.id.as_str()), is_hidden: a.hidden }).collect();
        self.visibility
            .borrow_mut()
            .apply_from(&policy, from_group, &entries, &mut |k| self.hide_key(&list, k), &mut |k| self.unhide_key(&list, k));
        let hidden = self.hidden_by_switch_ids(&list);
        self.state.borrow_mut().hidden_by_switch = hidden;
    }

    fn restore_visibility(&self) {
        let list = self.apps.running();
        let hidden: HashMap<String, bool> = list.iter().map(|a| (a.key(), a.hidden)).collect();
        self.visibility.borrow_mut().restore_all(&|k| hidden.get(k).copied().unwrap_or(false), &mut |k| self.unhide_key(&list, k));
        let hidden = self.hidden_by_switch_ids(&list);
        self.state.borrow_mut().hidden_by_switch = hidden;
    }

    // MARK: App guard

    pub fn app_activated(self: &Rc<Self>) {
        if !self.guards_apps.get() || !self.s().mode().blocks() {
            return;
        }
        let Some((app_id, window)) = self.apps.front() else { return };
        let list = self.apps.running();
        let Some(app) = list.iter().find(|a| a.id == app_id && a.windows.contains(&window)).cloned() else { return };
        let now = time::now();
        let name = self.apps.name(&app_id);
        if let Some(reason) = self.guard_reason(&app_id, now) {
            self.block_for(&app, &name, reason);
            return;
        }
        // Jev judges apps that are on no list. Apps in the group's dock are trusted.
        let (group, task, has_key) = {
            let s = self.s();
            let Some(g) = s.active_group() else { return };
            (g.clone(), s.current_task().map(|t| t.title.clone()), s.has_jev_key)
        };
        let allowed = self.s().ledger.is_allowed(&app_id, now);
        let Some(task) = task else { return };
        if !group.jev_enabled || !has_key || group.dock_apps.contains(&app_id) || appguard::never_guard(&app_id) || allowed {
            return;
        }
        let title = self.apps.window_title(window);
        let subject = privacy::app_subject(&name, &app_id, &title);
        let key = keys::get("typesafe");
        let jev = self.jev.clone();
        let tx = self.main.clone();
        std::thread::spawn(move || {
            let result = jev.check(key.as_deref(), &task, &group.name, &group.id.string(), &subject);
            let _ = tx.send_blocking(Box::new(move |m: &Rc<Model>| {
                let verdict = match result {
                    Ok(v) => {
                        m.set(change::KEYS, |s| s.last_jev_error = None);
                        v
                    }
                    Err(e) => {
                        m.set(change::KEYS, |s| s.last_jev_error = Some(e.to_string()));
                        return;
                    }
                };
                // Only act if the window Jev judged is still in front and we are still blocking.
                let still = m.apps.front().map(|(id, w)| id == app_id && w == window).unwrap_or(false) && m.apps.window_title(window) == title;
                if verdict.should_block(group.jev_threshold) && m.s().mode().blocks() && still && !m.s().ledger.is_allowed(&app_id, time::now()) {
                    let detail = format!("Jev thinks {name} is a distraction from your task.");
                    m.block(&app, &name, "jev", detail);
                }
            }));
        });
    }

    fn guard_reason(&self, app: &str, now: Date) -> Option<Reason> {
        let s = self.s();
        let ledger = &s.ledger;
        let allowed = |a: &str| ledger.is_allowed(a, now);
        let ctx = appguard::Context {
            mode: s.mode(),
            group: s.active_group(),
            own: Some("kairos.desktop"),
            shield_connected: s.shield_connected(),
            blocks_unshielded_browsers: s.data.settings.block_unshielded_browsers,
            is_allowed: &allowed,
        };
        ctx.reason(app)
    }

    fn sweep_soon(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(m) = weak.upgrade() {
                m.sweep();
            }
        });
    }

    /// Activation alone misses apps that are already open, so blocking starting, a group switch
    /// and an "Allow" running out check every visible app, and the frontmost one with Jev.
    fn sweep(self: &Rc<Self>) {
        if !self.guards_apps.get() || !self.s().mode().blocks() || self.s().active_group().is_none() {
            return;
        }
        let list = self.apps.running();
        let now = time::now();
        let hide: Vec<(String, Reason)> = {
            let s = self.s();
            let ledger = &s.ledger;
            let allowed = |a: &str| ledger.is_allowed(a, now);
            let ctx = appguard::Context {
                mode: s.mode(),
                group: s.active_group(),
                own: Some("kairos.desktop"),
                shield_connected: s.shield_connected(),
                blocks_unshielded_browsers: s.data.settings.block_unshielded_browsers,
                is_allowed: &allowed,
            };
            let apps: Vec<GuardApp> = list.iter().map(|a| GuardApp { app: a.id.clone(), visible: !a.hidden }).collect();
            ctx.sweep(&apps)
        };
        let front = self.apps.front().map(|f| f.0);
        // The frontmost app goes last, so the overlay names the app the owner was looking at.
        let ordered: Vec<&(String, Reason)> = hide.iter().filter(|h| Some(&h.0) != front.as_ref()).chain(hide.iter().filter(|h| Some(&h.0) == front.as_ref())).collect();
        for (id, reason) in ordered {
            for app in list.iter().filter(|a| &a.id == id && !a.hidden) {
                let name = self.apps.name(id);
                self.block_for(app, &name, *reason);
            }
        }
        if let Some(f) = front {
            if !hide.iter().any(|h| h.0 == f) {
                self.app_activated();
            }
        }
    }

    fn block_for(self: &Rc<Self>, app: &RunningApp, name: &str, reason: Reason) {
        let group = self.s().active_group().map(|g| g.name.clone()).unwrap_or_default();
        match reason {
            Reason::Listed => self.block(app, name, "app", format!("{name} is blocked in {group}.")),
            Reason::BrowserWithoutShield => {
                if browsers::is_chromium(&app.id) {
                    // Ask again once Shield has had time to connect from a browser that just opened.
                    let weak = Rc::downgrade(self);
                    let (key, id, name) = (app.key(), app.id.clone(), name.to_string());
                    glib::timeout_add_local_once(Duration::from_secs_f64(appguard::SHIELD_GRACE), move || {
                        let Some(m) = weak.upgrade() else { return };
                        let list = m.apps.running();
                        let Some(app) = list.iter().find(|a| a.key() == key && !a.hidden).cloned() else { return };
                        if m.guard_reason(&id, time::now()) == Some(Reason::BrowserWithoutShield) {
                            m.block_unshielded(&app, &name);
                        }
                    });
                    return;
                }
                self.block_unshielded(app, name);
            }
        }
    }

    fn block_unshielded(self: &Rc<Self>, app: &RunningApp, name: &str) {
        let why = if browsers::is_chromium(&app.id) {
            format!("Kairos Shield isn\u{2019}t connected, so your blocked sites would open in {name}.")
        } else {
            format!("Kairos Shield can\u{2019}t run in {name}, so your blocked sites would open there.")
        };
        self.block(app, name, "app", format!("{why} Use a Chromium browser with Kairos Shield, or turn this off in Settings > Blocking."));
    }

    /// Minimises, in Strict too: closing would lose the owner's unsaved work in that app.
    fn block(self: &Rc<Self>, app: &RunningApp, name: &str, layer: &str, detail: String) {
        let at = self.x.as_ref().and_then(|x| app.windows.iter().find(|w| !x.is_hidden(**w)).and_then(|w| x.geometry(*w))).map(|g| {
            let (x, y, w, h) = g.outer();
            (x + w / 2.0, y + h / 2.0)
        });
        self.apps.hide(app);
        let event = BlockEvent::new(self.s().data.focus.active_group_id, &app.id, Some(name.to_string()), layer, false, time::now());
        let now = time::now();
        self.edit(|d| d.log_block(event, now));
        self.show_notice(NoticeKind::App(app.id.clone()), format!("{name} is hidden"), detail, at);
    }

    fn show_notice(&self, kind: NoticeKind, title: String, detail: String, at: Option<(f64, f64)>) {
        let id = self.notice_count.get() + 1;
        self.notice_count.set(id);
        self.set(change::NOTICE, |s| s.notice = Some(BlockNotice { id, kind, title, detail, at }));
    }

    /// An "Allow" running out changes `temporaryAllows` and may leave a blocked app on screen.
    fn schedule_grant_expiry(self: &Rc<Self>) {
        if let Some(id) = self.grant_timer.borrow_mut().take() {
            id.remove();
        }
        let Some(next) = self.s().ledger.next_expiry(time::now()) else { return };
        let wait = (time::since(next, time::now()) + 0.05).max(0.2);
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_secs_f64(wait), move || {
            if let Some(m) = weak.upgrade() {
                m.grant_timer.borrow_mut().take();
                m.push_state(false);
                m.sweep();
                m.schedule_grant_expiry();
                m.emit(change::DATA);
            }
        });
        *self.grant_timer.borrow_mut() = Some(id);
    }

    /// "Allow N min" from the overlay. Returns an error message when refused.
    pub fn override_notice(self: &Rc<Self>, reason: &str) -> Option<String> {
        let target = match self.s().notice.as_ref().map(|n| n.kind.clone()) {
            Some(NoticeKind::App(id)) => id,
            _ => return None,
        };
        match self.request_override(&target, reason) {
            Ok(_) => {
                self.set(change::NOTICE, |s| s.notice = None);
                let running = self.s().running.clone();
                if let Err(e) = self.apps.open(&target, &running) {
                    eprintln!("Kairos: reopening {target} failed: {e}");
                }
                None
            }
            Err(r) => Some(r.message().into()),
        }
    }

    fn request_override(self: &Rc<Self>, target: &str, reason: &str) -> Result<Date, Refusal> {
        let result = {
            let mut s = self.state.borrow_mut();
            // Only something blocked right now can be allowed, so one request can't open a
            // whole suffix or every app.
            if !s.data.is_blocked_target(target, s.shield_connected()) {
                return Err(Refusal::NotBlocked);
            }
            let (mode, max, minutes) = (s.mode(), s.data.settings.max_overrides_per_session, s.data.settings.override_minutes);
            s.ledger.request(target, reason, mode, max, minutes, time::now())
        };
        if result.is_ok() {
            let t = target.to_string();
            self.edit(|d| d.mark_overridden(&t));
            self.push_state(false);
            self.schedule_grant_expiry();
        }
        result
    }

    pub fn dismiss_notice(self: &Rc<Self>) {
        let rest = self.s().notice.as_ref().map(|n| n.kind == NoticeKind::Rest).unwrap_or(false);
        if rest {
            self.request_skip();
        }
        self.set(change::NOTICE, |s| s.notice = None);
    }

    pub fn hide_notice(&self) {
        self.set(change::NOTICE, |s| s.notice = None);
    }

    // MARK: Bridge

    pub fn push_state(&self, force: bool) {
        let message = {
            let s = self.s();
            json!({"type": "state", "state": rules::bridge_state(&s.data, &s.ledger, s.cloud_voice_ready(), time::now())})
        };
        let encoded = message.to_string();
        if !force && *self.last_pushed.borrow() == encoded {
            return;
        }
        *self.last_pushed.borrow_mut() = encoded;
        self.bridge.broadcast(&message, None);
    }

    pub fn send_read_command(&self, command: &str) {
        self.bridge.broadcast(&json!({"type": "read.command", "command": command}), Some("read"));
    }

    fn handle(self: &Rc<Self>, m: serde_json::Map<String, Value>, client: ClientId) {
        let Some(kind) = m.get("type").and_then(|v| v.as_str()).map(String::from) else { return };
        let id = m.get("id").and_then(|v| v.as_str()).map(String::from);
        let str_of = |k: &str| m.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let reply = Reply { bridge: self.bridge.clone(), client, id: id.clone() };
        let state_message = || {
            let s = self.s();
            json!({"type": "state", "state": rules::bridge_state(&s.data, &s.ledger, s.cloud_voice_ready(), time::now())})
        };
        let never = self.s().data.settings.never_analyse.clone();
        if let Some(refusal) = privacy::refusal(&m, &never) {
            // tts.prefetch has no id and expects no answer, so it is simply dropped.
            if id.is_some() {
                reply.send(json!({"type": "error", "message": refusal}));
            }
            return;
        }
        let locked = self.s().strict_locked();
        match kind.as_str() {
            "hello" => {
                let name = str_of("client");
                self.bridge.set_name(&name, client);
                if (name == "shield" || name == "read") && !self.s().data.seen_clients.contains(&name) {
                    self.edit(|d| d.seen_clients.push(name.clone()));
                }
                reply.send(state_message());
            }
            "getState" => reply.send(state_message()),
            "app.show" => {
                let show = self.hooks.borrow().show_main_window.clone();
                if let Some(f) = show {
                    f();
                }
            }
            "blockEvent" => {
                let event = BlockEvent::new(
                    self.s().data.focus.active_group_id,
                    &rules::host_of(&str_of("target")),
                    None,
                    &str_of("layer"),
                    m.get("overridden").and_then(|v| v.as_bool()).unwrap_or(false),
                    time::now(),
                );
                let now = time::now();
                self.edit(|d| d.log_block(event, now));
            }
            "override.request" => {
                let result = self.request_override(&rules::normalize_site(&str_of("target")), &str_of("reason"));
                let left = self.s().overrides_left();
                match result {
                    Ok(until) => reply.send(json!({"type": "override.result", "allowed": true, "until": time::ms(until), "remaining": left})),
                    Err(r) => reply.send(json!({"type": "override.result", "allowed": false, "remaining": left, "message": r.message()})),
                }
            }
            "rule.addElement" => {
                let (sel, host) = (str_of("selector"), str_of("host"));
                self.edit(|d| {
                    d.set_element_hidden(&sel, &host, true, locked);
                });
            }
            "rule.addSite" => {
                let Some(list) = SiteList::parse(&str_of("list")) else { return };
                let site = str_of("site");
                let mut ok = false;
                self.edit(|d| ok = d.add_site(&site, list, locked));
                // A refused allow (Strict) changes nothing; the push tells the extension so.
                if !ok {
                    self.push_state(true);
                }
            }
            "rule.removeElement" => {
                let (sel, host) = (str_of("selector"), str_of("host"));
                let mut ok = false;
                self.edit(|d| ok = d.set_element_hidden(&sel, &host, false, locked));
                if !ok {
                    self.push_state(true);
                }
            }
            "read.status" => {
                let state = str_of("state");
                let title = str_of("title");
                self.set(change::CLIENTS, |s| s.read_status = if state == "stopped" { None } else { Some(ReadStatus { state, title }) });
            }
            "read.log" => {
                let secs = m.get("seconds").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64;
                self.edit(|d| d.log_listening(secs));
            }
            "tts.speak" => {
                let rate = m.get("rate").and_then(|v| v.as_f64()).unwrap_or(1.0);
                self.speak(str_of("text"), rate, reply);
            }
            "tts.prefetch" => {
                let text = str_of("text");
                let (ready, provider, voice) = {
                    let s = self.s();
                    (s.cloud_voice_ready(), s.data.settings.voice_provider, s.data.settings.voice_id.clone())
                };
                let Some(account) = provider.key_account() else { return };
                if !ready || text.is_empty() {
                    return;
                }
                let key = keys::get(account);
                let voices = self.voices.clone();
                std::thread::spawn(move || {
                    let _ = voices.synthesize(provider, key.as_deref(), &voice, &text);
                });
            }
            "tts.stop" => {
                self.speech_generation.set(self.speech_generation.get() + 1);
                self.speech.stop();
            }
            "tts.pause" => self.speech.pause(),
            "tts.resume" => self.speech.resume(),
            "jev.check" => self.jev_check(&m, reply),
            "scene.candidates" | "scene.generate" | "scene.edit" => self.scene_request(&kind, &m, reply),
            "scene.cancel" => {
                if let Some(id) = id {
                    if let Some(c) = self.scene_tasks.borrow_mut().remove(&id) {
                        c.cancel();
                    }
                }
            }
            _ => {}
        }
    }

    /// Speaks one sentence with the chosen cloud voice and replies `tts.done` when it ends.
    fn speak(self: &Rc<Self>, text: String, rate: f64, reply: Reply) {
        let (ready, provider, voice) = {
            let s = self.s();
            (s.cloud_voice_ready(), s.data.settings.voice_provider, s.data.settings.voice_id.clone())
        };
        let Some(account) = provider.key_account().filter(|_| ready) else {
            return reply.send(json!({"type": "error", "message": "No cloud voice is set up in Kairos."}));
        };
        self.speech.stop();
        let generation = self.speech_generation.get() + 1;
        self.speech_generation.set(generation);
        let key = keys::get(account);
        let voices = self.voices.clone();
        let tx = self.main.clone();
        std::thread::spawn(move || {
            let result = voices.synthesize(provider, key.as_deref(), &voice, &text);
            let _ = tx.send_blocking(Box::new(move |m: &Rc<Model>| match result {
                Ok(audio) => {
                    // A stop or a newer sentence arrived while this one was being fetched.
                    if generation != m.speech_generation.get() {
                        return reply.send(json!({"type": "tts.done", "stopped": true}));
                    }
                    let r = reply.clone();
                    match m.speech.play(&audio, rate, move |stopped| r.send(json!({"type": "tts.done", "stopped": stopped}))) {
                        Ok(()) => m.set(change::KEYS, |s| s.last_voice_error = None),
                        Err(e) => {
                            m.set(change::KEYS, |s| s.last_voice_error = Some(e.clone()));
                            reply.send(json!({"type": "error", "message": e}));
                        }
                    }
                }
                Err(e) => {
                    m.set(change::KEYS, |s| s.last_voice_error = Some(e.to_string()));
                    reply.send(json!({"type": "error", "message": e.to_string()}));
                }
            }));
        });
    }

    fn jev_check(self: &Rc<Self>, m: &serde_json::Map<String, Value>, reply: Reply) {
        let unsure = |reply: &Reply, block: bool| {
            reply.send(json!({"type": "jev.result", "choice": "unsure", "confidence": 0, "probabilities": {}, "block": block, "cached": false}))
        };
        let get = |k: &str| m.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let url = get("url");
        let host = rules::host_of(&url);
        let (group, task, fail_closed) = {
            let s = self.s();
            let ok = s.mode().blocks() && !rules::host_in(&host, &s.data.settings.never_analyse) && !s.ledger.is_allowed(&host, time::now());
            match (s.active_group().filter(|g| g.jev_enabled && ok), s.current_task()) {
                (Some(g), Some(t)) => (g.clone(), t.title.clone(), s.data.settings.jev_fail_closed),
                _ => return unsure(&reply, false),
            }
        };
        let mut subject = Subject::new("page");
        subject.url = url;
        subject.title = get("title");
        subject.description = get("description");
        subject.excerpt = get("excerpt");
        let key = keys::get("typesafe");
        let jev = self.jev.clone();
        let tx = self.main.clone();
        std::thread::spawn(move || {
            let result = jev.check(key.as_deref(), &task, &group.name, &group.id.string(), &subject);
            let _ = tx.send_blocking(Box::new(move |m: &Rc<Model>| match result {
                Ok(v) => {
                    m.set(change::KEYS, |s| s.last_jev_error = None);
                    reply.send(json!({"type": "jev.result", "choice": v.choice, "confidence": v.confidence, "probabilities": v.probabilities,
                                      "block": v.should_block(group.jev_threshold), "cached": v.cached}));
                }
                Err(e) => {
                    m.set(change::KEYS, |s| s.last_jev_error = Some(e.to_string()));
                    // No key means Jev is simply not set up: never block on that.
                    let block = !matches!(e, JevError::NoKey) && fail_closed;
                    unsure(&reply, block);
                }
            }));
        });
    }

    /// Chrome drops the port on a host message over 1 MB, so an oversized scene becomes an error.
    fn scene_reply(html: String) -> Value {
        if html.len() > 900_000 {
            return json!({"type": "error", "message": "That scene came out too large to send to Chrome (over 900 KB). Try a simpler subject."});
        }
        json!({"type": "scene.result", "html": html})
    }

    fn scene_request(self: &Rc<Self>, kind: &str, m: &serde_json::Map<String, Value>, reply: Reply) {
        let get = |k: &str| m.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let (title, text, subject, html, instruction) = (get("title"), get("text"), get("subject"), get("html"), get("instruction"));
        let id = m.get("id").and_then(|v| v.as_str()).map(String::from);
        let (choice, known_node) = {
            let s = self.s();
            (s.data.settings.scene_backend, s.scene_setup.node.clone())
        };
        let api_key = if choice == SceneBackend::ApiKey { keys::get("anthropic") } else { None };
        let cancel = Cancel::default();
        if let Some(id) = &id {
            self.scene_tasks.borrow_mut().insert(id.clone(), cancel.clone());
        }
        let kind = kind.to_string();
        let resources = system::resources();
        let tx = self.main.clone();
        std::thread::spawn(move || {
            let writer = SceneWriter {
                backend: match choice {
                    SceneBackend::Subscription => Box::new(ClaudeSubscription::new(known_node.or_else(node::locate), node::helper_url(resources.as_deref()))),
                    SceneBackend::ApiKey => Box::new(ClaudeApi { api_key }),
                },
            };
            let answer = match kind.as_str() {
                "scene.candidates" => writer.candidates(&title, &text, &cancel).map(|list| {
                    json!({"type": "scene.candidates.result", "candidates": list.iter().map(|c| json!({"name": c.name, "kind": c.kind, "why": c.why})).collect::<Vec<_>>()})
                }),
                "scene.generate" => writer.generate(&title, &text, &subject, &cancel).map(Model::scene_reply),
                _ => writer.edit(&html, &instruction, &cancel).map(Model::scene_reply),
            };
            reply.send(answer.unwrap_or_else(|e| json!({"type": "error", "message": e.to_string()})));
            let _ = tx.send_blocking(Box::new(move |m: &Rc<Model>| {
                if let Some(id) = id {
                    m.scene_tasks.borrow_mut().remove(&id);
                }
            }));
        });
    }
}

pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
