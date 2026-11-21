//! Kairos for Linux Mint (Cinnamon on X11): the app's entry point. Wires the model to the main
//! window, Kairos Bar, the tray icon, global shortcuts, notifications and Cinnamon's panel.

mod model;
mod platform;
mod ui;

use gtk::prelude::*;
use gtk::{gio, glib};
use kairos_core::hotkeys::{Action, HotkeyPlan};
use kairos_core::tempo::TempoPhase;
use kairos_core::{panel, store, Mode};
use model::{change, Model};
use platform::system::bridge_installer;
use platform::tray::{KairosTray, TrayCommand, TraySnapshot};
use platform::x11::{WmEvent, X11};
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::rc::Rc;
use std::time::Duration;
use ui::window::MainWindow;

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--install-bridge") {
        let done = bridge_installer::install();
        println!("{}", if done.is_empty() { "No browser found.".to_string() } else { format!("Installed for {}", done.join(", ")) });
        return glib::ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--restore-panel") {
        panel::restore();
        return glib::ExitCode::SUCCESS;
    }
    // One Kairos at a time: a second launch asks the first to show its window.
    if !take_instance_lock() {
        show_running_instance();
        return glib::ExitCode::SUCCESS;
    }
    // Kairos draws simple widgets; GTK's cairo renderer uses about a third less memory than GL.
    if std::env::var_os("GSK_RENDERER").is_none() {
        std::env::set_var("GSK_RENDERER", "cairo");
    }
    gstreamer::init().ok();
    let app = gtk::Application::builder().application_id("com.kairos.desktop").flags(gio::ApplicationFlags::NON_UNIQUE).build();
    app.connect_startup(|_| {
        let provider = gtk::CssProvider::new();
        provider.load_from_string(ui::widgets::CSS);
        if let Some(d) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&d, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
    });
    let started = Cell::new(false);
    app.connect_activate(move |app| {
        if started.replace(true) {
            return;
        }
        start(app);
    });
    app.run_with_args::<&str>(&[])
}

/// Takes the lock that marks this as the one running Kairos for its data folder, and holds it
/// until the process ends (the system releases it on a crash too). The bridge socket alone was
/// not enough: it is bound well into startup, so launches close together all saw no socket and
/// all started. False when another Kairos holds the lock.
fn take_instance_lock() -> bool {
    let dir = kairos_core::store::default_directory();
    let _ = std::fs::create_dir_all(&dir);
    let Ok(file) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("kairos.lock")) else {
        return true; // Can't make a lock file: run rather than refuse to start.
    };
    match file.try_lock() {
        Ok(()) => {
            std::mem::forget(file); // Held for the life of the process.
            true
        }
        Err(std::fs::TryLockError::WouldBlock) => false,
        Err(_) => true,
    }
}

/// Asks the running Kairos to show its window. It may still be starting and not yet listening,
/// so this waits a few seconds for its socket.
fn show_running_instance() {
    let socket = kairos_core::bridge::socket_path();
    for _ in 0..50 {
        if let Ok(mut s) = std::os::unix::net::UnixStream::connect(&socket) {
            let _ = s.write_all(b"{\"type\":\"app.show\"}\n");
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

struct App {
    app: gtk::Application,
    model: Rc<Model>,
    window: Rc<MainWindow>,
    bar: Option<Rc<ui::bar::KairosBar>>,
    x: Option<std::sync::Arc<X11>>,
    hotkeys: RefCell<Vec<Action>>,
    hotkey_plan: RefCell<Option<HotkeyPlan>>,
    tray: RefCell<Option<ksni::Handle<KairosTray>>>,
    tray_snap: RefCell<TraySnapshot>,
    panel_hidden: Cell<bool>,
    _hold: gio::ApplicationHoldGuard,
}

fn start(app: &gtk::Application) {
    let x = X11::connect();
    if x.is_none() {
        eprintln!("Kairos: no X11 display. Kairos Bar, app blocking and shortcuts need Cinnamon on X11.");
    }
    let model = Model::new(x.clone());
    let window = MainWindow::new(app, &model);
    let bar = x.as_ref().map(|x| ui::bar::KairosBar::new(app, &model, x.clone()));
    let _overlay = ui::overlay::Overlay::new(app, &model, x.clone());
    let me = Rc::new(App {
        app: app.clone(),
        model: model.clone(),
        window,
        bar,
        x: x.clone(),
        hotkeys: RefCell::new(vec![]),
        hotkey_plan: RefCell::new(None),
        tray: RefCell::new(None),
        tray_snap: RefCell::new(TraySnapshot::default()),
        panel_hidden: Cell::new(false),
        _hold: app.hold(),
    });
    // Keeps the overlay alive for the life of the app.
    std::mem::forget(_overlay);
    install_hooks(&me);
    // Window manager events.
    if let Some(x) = &x {
        let tx = model.sender();
        let weak_app = Rc::downgrade(&me);
        let (etx, erx) = async_channel::unbounded::<WmEvent>();
        x.watch(move |e| {
            let _ = etx.send_blocking(e);
        });
        let _ = tx;
        glib::spawn_future_local(async move {
            // Bursts of window events are folded into one refresh.
            let pending = Rc::new(Cell::new(false));
            while let Ok(e) = erx.recv().await {
                let Some(me) = weak_app.upgrade() else { break };
                match e {
                    WmEvent::ActiveChanged => {
                        me.model.app_activated();
                        if let Some(b) = &me.bar {
                            b.active_window_changed();
                        }
                        refresh_soon(&me, &pending);
                    }
                    WmEvent::ClientsChanged | WmEvent::WindowChanged(_) => {
                        refresh_soon(&me, &pending);
                        if let Some(b) = &me.bar {
                            b.active_window_changed();
                        }
                    }
                    WmEvent::DesktopChanged => {}
                    WmEvent::Hotkey(i) => {
                        let action = me.hotkeys.borrow().get(i).copied();
                        match action {
                            Some(Action::QuickAdd) => ui::quickadd::show(&me.app, &me.model),
                            Some(Action::Timer) => me.model.toggle_timer(),
                            Some(Action::Group(n)) => me.model.switch_group_index(n),
                            None => {}
                        }
                    }
                }
            }
        });
    }
    // Reacting to settings.
    {
        let weak = Rc::downgrade(&me);
        model.subscribe(move |what| {
            let Some(me) = weak.upgrade() else { return };
            if what & change::DATA != 0 {
                me.grab_hotkeys();
                me.sync_panel();
            }
            if what & !change::NAV != 0 {
                me.update_tray();
            }
        });
    }
    model.start();
    me.grab_hotkeys();
    me.sync_panel();
    me.start_tray();
    // A first run installs the browser link, so the extensions connect as soon as they load.
    if bridge_installer::installed_browsers().is_empty() {
        let done = bridge_installer::install();
        if !done.is_empty() {
            eprintln!("Kairos: installed the browser link for {}", done.join(", "));
        }
    }
    let first = !model.s().data.onboarded;
    if first || std::env::var("KAIROS_SHOW").is_ok() || !std::env::args().any(|a| a == "--background") {
        me.window.present();
    }
    // For testing and screenshots: KAIROS_PAGE=settings opens that page.
    if let Ok(key) = std::env::var("KAIROS_PAGE") {
        if let Some(p) = model::Page::ALL.iter().find(|p| p.key() == key) {
            let p = *p;
            model.set(change::NAV, |s| s.page = p);
        }
    }
    if std::env::var("KAIROS_START_FOCUS").is_ok() {
        model.start_focus();
    }
    // Quitting: Ctrl+C or a logout puts the panel back and saves.
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        let weak = Rc::downgrade(&me);
        glib::unix_signal_add_local(signal, move || {
            if let Some(me) = weak.upgrade() {
                me.quit(true);
            }
            glib::ControlFlow::Break
        });
    }
    // The App lives as long as the GTK application.
    let keep = me.clone();
    app.connect_shutdown(move |_| {
        keep.finish();
    });
}

fn refresh_soon(me: &Rc<App>, pending: &Rc<Cell<bool>>) {
    if pending.replace(true) {
        return;
    }
    let (weak, pending) = (Rc::downgrade(me), pending.clone());
    glib::timeout_add_local_once(Duration::from_millis(60), move || {
        pending.set(false);
        if let Some(me) = weak.upgrade() {
            me.model.refresh_running();
        }
    });
}

fn install_hooks(me: &Rc<App>) {
    let mut hooks = me.model.hooks.borrow_mut();
    {
        let weak = Rc::downgrade(me);
        hooks.show_main_window = Some(Rc::new(move || {
            if let Some(me) = weak.upgrade() {
                me.show_window();
            }
        }));
    }
    {
        let app = me.app.clone();
        hooks.notify = Some(Rc::new(move |id, title, body| {
            let n = gio::Notification::new(title);
            n.set_body(Some(body));
            n.set_default_action("app.show");
            app.send_notification(id, &n);
        }));
    }
    {
        let app = me.app.clone();
        hooks.withdraw_notification = Some(Rc::new(move |id| app.withdraw_notification(id)));
    }
    {
        let model = me.model.clone();
        hooks.confirm_strict = Some(Rc::new(move |c, then| {
            let then = RefCell::new(Some(then));
            let parent = ui::widgets::main_window(&model);
            ui::widgets::ask(parent.as_ref(), &c.title, &c.message, &["Start strict focus", "Cancel"], 1, move |r| {
                if r == Some(0) {
                    if let Some(f) = then.borrow_mut().take() {
                        f();
                    }
                }
            });
        }));
    }
    drop(hooks);
    let show = gio::SimpleAction::new("show", None);
    let weak = Rc::downgrade(me);
    show.connect_activate(move |_, _| {
        if let Some(me) = weak.upgrade() {
            me.show_window();
        }
    });
    me.app.add_action(&show);
}

impl App {
    /// Shows the main window and brings it forward. present() alone is not enough for an
    /// already-open window: without a user timestamp Muffin's focus-stealing prevention only
    /// marks it as wanting attention, which shows nowhere while Kairos hides the panel. So the
    /// window is also activated as a pager would, which Muffin obeys, after moving it to the
    /// workspace on screen (Muffin ignores activation across workspaces). A window that was hidden
    /// is mapped a moment after present(), so it is activated again then.
    fn show_window(self: &Rc<Self>) {
        self.window.present();
        self.activate_window();
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(150), move || {
            if let Some(me) = weak.upgrade() {
                me.activate_window();
            }
        });
    }

    fn activate_window(&self) {
        let Some(x) = &self.x else { return };
        let surface = self.window.window.surface().and_then(|s| s.downcast::<gdk4_x11::X11Surface>().ok());
        if let Some(xid) = surface.map(|s| s.xid() as u32) {
            // Muffin won't activate a window on another workspace, so it comes to this one.
            x.move_to_current_desktop(xid);
            x.activate(xid);
        }
    }

    fn grab_hotkeys(&self) {
        let Some(x) = &self.x else { return };
        let plan = HotkeyPlan::new(&self.model.s().data.settings);
        if self.hotkey_plan.borrow().as_ref() == Some(&plan) {
            return;
        }
        let keys: Vec<(u32, u16)> = plan.entries.iter().map(|e| (e.keysym, e.modifiers)).collect();
        let failed = x.grab_hotkeys(&keys);
        let labels: Vec<String> = failed.iter().filter_map(|i| plan.entries.get(*i)).map(|e| e.label.clone()).collect();
        *self.hotkeys.borrow_mut() = plan.entries.iter().map(|e| e.action).collect();
        *self.hotkey_plan.borrow_mut() = Some(plan);
        if self.model.s().hotkey_failures != labels {
            self.model.set(change::KEYS, |s| s.hotkey_failures = labels);
        }
    }

    /// Hides Cinnamon's panel while Kairos Bar takes its place, and puts it back otherwise.
    fn sync_panel(&self) {
        if store::is_scratch() {
            return;
        }
        let want = self.model.s().data.settings.hides_system_dock() && self.x.is_some();
        if want == self.panel_hidden.get() && (want == panel::is_replaced()) {
            return;
        }
        if want {
            match panel::replace() {
                Ok(()) => self.panel_hidden.set(true),
                Err(e) => eprintln!("Kairos: could not hide Cinnamon's panel: {e:?}"),
            }
        } else {
            panel::restore();
            self.panel_hidden.set(false);
        }
        if let Some(b) = &self.bar {
            let b = b.clone();
            glib::timeout_add_local_once(Duration::from_millis(400), move || b.refresh());
        }
    }

    fn snapshot(&self) -> TraySnapshot {
        let s = self.model.s();
        let idle = s.tempo.phase() == TempoPhase::Idle;
        let active = s.active_group();
        let color = active.map(|g| ui::icons::rgb8(&g.color_hex)).unwrap_or((0x4f, 0x7c, 0xff));
        let quick = kairos_core::hotkeys::choice(&s.data.settings.quick_add_hotkey, kairos_core::hotkeys::QUICK_ADD).map(|c| c.label.to_string());
        let timer = kairos_core::hotkeys::choice(&s.data.settings.timer_hotkey, kairos_core::hotkeys::TIMER).map(|c| c.label.to_string());
        TraySnapshot {
            title: s.menu_title(),
            color,
            progress: if idle { None } else { Some(s.tempo.progress(s.now)) },
            blocking_off: s.site_blocking_off(),
            current_task: s.current_task().map(|t| t.title.clone()),
            idle,
            awaiting: s.tempo.awaiting_focus(),
            phase_line: if idle {
                String::new()
            } else {
                format!("{} \u{00B7} {} left{}", s.tempo.phase().label(), kairos_core::tempo::clock(s.remaining()), if s.tempo.is_paused() { ", paused" } else { "" })
            },
            paused: s.tempo.is_paused(),
            strict_locked: s.strict_locked(),
            reading: s.read_status.as_ref().map(|r| r.state == "playing"),
            groups: s.data.groups.iter().map(|g| g.name.clone()).collect(),
            active_group: s.data.groups.iter().position(|g| Some(g.id) == s.data.focus.active_group_id).unwrap_or(usize::MAX),
            mode: Mode::ALL.iter().position(|m| *m == s.mode()).unwrap_or(0),
            mode_locked: !idle,
            quick_add_key: quick,
            timer_key: timer,
            bar_visible: s.data.settings.dock_visible,
            quit_note: if s.strict_locked() { Some("Strict focus is on. Kairos can\u{2019}t quit until it ends.".into()) } else { None },
        }
    }

    fn update_tray(&self) {
        let snap = self.snapshot();
        if *self.tray_snap.borrow() == snap {
            return;
        }
        *self.tray_snap.borrow_mut() = snap.clone();
        if let Some(h) = self.tray.borrow().as_ref() {
            h.update(move |t| t.snap = snap.clone());
        }
    }

    fn start_tray(self: &Rc<Self>) {
        let (tx, rx) = async_channel::unbounded::<TrayCommand>();
        let snap = self.snapshot();
        *self.tray_snap.borrow_mut() = snap.clone();
        let service = ksni::TrayService::new(KairosTray { snap, send: tx });
        *self.tray.borrow_mut() = Some(service.handle());
        service.spawn();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(c) = rx.recv().await {
                let Some(me) = weak.upgrade() else { break };
                me.tray_command(c);
            }
        });
    }

    fn tray_command(self: &Rc<Self>, c: TrayCommand) {
        let m = &self.model;
        match c {
            TrayCommand::MarkDone => {
                let id = m.s().current_task().map(|t| t.id);
                if let Some(id) = id {
                    m.set_done(id, true);
                }
            }
            TrayCommand::StartFocus => m.request_start_focus(),
            TrayCommand::NotNow => m.end_break_wait(),
            TrayCommand::ToggleTimer => m.toggle_timer(),
            TrayCommand::Skip => m.request_skip(),
            TrayCommand::AddFive => m.add_five_minutes(),
            TrayCommand::Stop => m.stop_timer(),
            TrayCommand::ReadToggle => m.send_read_command("toggle"),
            TrayCommand::SwitchGroup(i) => m.switch_group_index(i),
            TrayCommand::NewGroup => ui::widgets::actions::new_group(m),
            TrayCommand::SetMode(mode) => m.set_mode(mode),
            TrayCommand::QuickAdd => ui::quickadd::show(&self.app, m),
            TrayCommand::Open => self.show_window(),
            TrayCommand::ToggleBar => m.edit(|d| d.settings.dock_visible = !d.settings.dock_visible),
            TrayCommand::Quit => self.quit(false),
        }
    }

    /// Quits unless strict focus is on. `signal` quits anyway (a logout cannot be refused), but
    /// strict focus picks up again at the next launch.
    fn quit(&self, signal: bool) {
        if !signal && self.model.s().strict_locked() {
            ui::widgets::alert(ui::widgets::main_window(&self.model).as_ref(), "Strict focus is on", "Kairos can\u{2019}t quit until this focus session ends.");
            return;
        }
        self.app.quit();
    }

    fn finish(&self) {
        if self.panel_hidden.get() {
            panel::restore();
            self.panel_hidden.set(false);
        }
        self.model.shutdown();
        if let Some(h) = self.tray.borrow_mut().take() {
            h.shutdown();
        }
    }
}
