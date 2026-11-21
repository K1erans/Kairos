//! Apps as Kairos sees them: desktop entries (an app's id is its .desktop file name, as in the
//! menu) and the windows they have open. A running app is the set of taskbar windows that
//! resolve to one desktop id and process.

use super::x11::X11;
use gtk::gio;
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub struct RunningApp {
    pub id: String,
    pub pid: u32,
    pub windows: Vec<u32>,
    /// Every window minimised.
    pub hidden: bool,
}

impl RunningApp {
    /// Identifies this run of the app, so a relaunch counts as a new app (GroupVisibility).
    pub fn key(&self) -> String {
        format!("{}#{}", self.id, self.pid)
    }
}

#[derive(Clone, Debug)]
pub struct InstalledApp {
    pub id: String,
    pub name: String,
    pub icon: Option<gio::Icon>,
}

#[derive(Default)]
struct Index {
    /// Lower-cased StartupWMClass, id stem and executable name, to a desktop id.
    by_class: HashMap<String, String>,
    built: Option<Instant>,
}

pub struct Apps {
    pub x: Option<Arc<X11>>,
    own_pid: u32,
    index: RefCell<Index>,
    resolved: RefCell<HashMap<u32, String>>,
    names: RefCell<HashMap<String, String>>,
}

impl Apps {
    pub fn new(x: Option<Arc<X11>>) -> Self {
        Apps { x, own_pid: std::process::id(), index: RefCell::default(), resolved: RefCell::default(), names: RefCell::default() }
    }

    pub fn info(id: &str) -> Option<gio::DesktopAppInfo> {
        gio::DesktopAppInfo::new(id)
    }

    pub fn is_installed(id: &str) -> bool {
        Self::info(id).is_some()
    }

    pub fn name(&self, id: &str) -> String {
        if let Some(n) = self.names.borrow().get(id) {
            return n.clone();
        }
        let n = Self::info(id).map(|i| i.display_name().to_string()).unwrap_or_else(|| stem(id));
        self.names.borrow_mut().insert(id.to_string(), n.clone());
        n
    }

    pub fn icon(id: &str) -> Option<gio::Icon> {
        Self::info(id).and_then(|i| i.icon())
    }

    /// Every app the menu shows, by name.
    pub fn installed() -> Vec<InstalledApp> {
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<InstalledApp> = gio::AppInfo::all()
            .into_iter()
            .filter(|a| a.should_show())
            .filter_map(|a| {
                let id = a.id()?.to_string();
                seen.insert(id.clone()).then(|| InstalledApp { id, name: a.display_name().to_string(), icon: a.icon() })
            })
            .collect();
        out.sort_by_key(|a| a.name.to_lowercase());
        out
    }

    /// The desktop id of a .desktop file dropped on Kairos Bar.
    pub fn id_for_file(path: &std::path::Path) -> Option<String> {
        let info = gio::DesktopAppInfo::from_filename(path)?;
        info.id().map(|s| s.to_string()).or_else(|| path.file_name().map(|n| n.to_string_lossy().to_string()))
    }

    fn rebuild_index(&self) {
        let mut idx = self.index.borrow_mut();
        if idx.built.map(|t| t.elapsed().as_secs() < 30).unwrap_or(false) {
            return;
        }
        idx.by_class.clear();
        for app in gio::AppInfo::all() {
            let Some(id) = app.id().map(|s| s.to_string()) else { continue };
            let Ok(info) = app.clone().downcast::<gio::DesktopAppInfo>() else { continue };
            if let Some(class) = info.startup_wm_class() {
                idx.by_class.insert(class.to_lowercase(), id.clone());
            }
            idx.by_class.entry(stem(&id).to_lowercase()).or_insert_with(|| id.clone());
            // "org.gnome.Nautilus" also answers to "nautilus".
            if let Some(last) = stem(&id).rsplit('.').next() {
                idx.by_class.entry(last.to_lowercase()).or_insert_with(|| id.clone());
            }
            // executable() panics on entries without Exec in this gio version; read Exec instead.
            let exe = info.string("Exec").map(|e| e.to_string()).unwrap_or_default();
            let first = exe.split_whitespace().find(|w| !w.contains('=') && *w != "env").unwrap_or("");
            if let Some(name) = std::path::Path::new(first.trim_matches('"')).file_name() {
                idx.by_class.entry(name.to_string_lossy().to_lowercase()).or_insert_with(|| id.clone());
            }
        }
        idx.built = Some(Instant::now());
    }

    fn launched_desktop_file(pid: u32) -> Option<String> {
        let env = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
        let mut file = None;
        let mut for_pid = None;
        for entry in env.split(|b| *b == 0) {
            let s = String::from_utf8_lossy(entry);
            if let Some(v) = s.strip_prefix("GIO_LAUNCHED_DESKTOP_FILE=") {
                file = Some(v.to_string());
            } else if let Some(v) = s.strip_prefix("GIO_LAUNCHED_DESKTOP_FILE_PID=") {
                for_pid = v.parse::<u32>().ok();
            }
        }
        // Children inherit the variable from whatever started them, so only trust it for the
        // process it was set for.
        if for_pid != Some(pid) {
            return None;
        }
        let path = std::path::PathBuf::from(file?);
        Self::id_for_file(&path)
    }

    /// The desktop id a window belongs to.
    pub fn app_id(&self, w: u32) -> Option<String> {
        if let Some(id) = self.resolved.borrow().get(&w) {
            return Some(id.clone());
        }
        let x = self.x.as_ref()?;
        let id = self.resolve(x, w)?;
        self.resolved.borrow_mut().insert(w, id.clone());
        Some(id)
    }

    fn resolve(&self, x: &X11, w: u32) -> Option<String> {
        if let Some(gtk_id) = x.gtk_app_id(w) {
            let id = format!("{gtk_id}.desktop");
            if Self::is_installed(&id) {
                return Some(id);
            }
        }
        if let Some(id) = x.pid(w).and_then(Self::launched_desktop_file) {
            return Some(id);
        }
        let (instance, class) = x.wm_class(w);
        if instance.is_empty() && class.is_empty() {
            return None;
        }
        for attempt in 0..2 {
            if attempt == 1 {
                // An app installed since the index was built.
                self.index.borrow_mut().built = None;
            }
            self.rebuild_index();
            let idx = self.index.borrow();
            for key in [&class, &instance] {
                if let Some(id) = idx.by_class.get(&key.to_lowercase()) {
                    return Some(id.clone());
                }
            }
        }
        Some(format!("{}.desktop", if class.is_empty() { instance } else { class }.to_lowercase()))
    }

    /// Open apps with taskbar windows, in the order their first window appeared, without Kairos.
    pub fn running(&self) -> Vec<RunningApp> {
        let Some(x) = &self.x else { return vec![] };
        let list = x.client_list();
        self.resolved.borrow_mut().retain(|w, _| list.contains(w));
        let mut out: Vec<RunningApp> = vec![];
        for w in list {
            if !x.is_app_window(w) {
                continue;
            }
            let pid = x.pid(w).unwrap_or(0);
            if pid == self.own_pid {
                continue;
            }
            let Some(id) = self.app_id(w) else { continue };
            let hidden = x.is_hidden(w);
            match out.iter_mut().find(|a| a.id == id && a.pid == pid) {
                Some(app) => {
                    app.windows.push(w);
                    app.hidden &= hidden;
                }
                None => out.push(RunningApp { id, pid, windows: vec![w], hidden }),
            }
        }
        out
    }

    pub fn front(&self) -> Option<(String, u32)> {
        let x = self.x.as_ref()?;
        let w = x.active_window()?;
        if x.pid(w) == Some(self.own_pid) {
            return None;
        }
        Some((self.app_id(w)?, w))
    }

    pub fn hide(&self, app: &RunningApp) -> bool {
        let Some(x) = &self.x else { return false };
        for w in &app.windows {
            x.minimize(*w);
        }
        true
    }

    pub fn unhide(&self, app: &RunningApp) -> bool {
        let Some(x) = &self.x else { return false };
        for w in &app.windows {
            x.activate(*w);
        }
        true
    }

    pub fn quit(&self, app: &RunningApp) {
        if let Some(x) = &self.x {
            for w in &app.windows {
                x.close(*w);
            }
        }
    }

    /// Opens an app the way a Dock click does: a running app comes forward with its windows,
    /// otherwise it starts. False when it could not start.
    pub fn open(&self, id: &str, running: &[RunningApp]) -> Result<(), String> {
        if let Some(app) = running.iter().find(|a| a.id == id) {
            if let Some(x) = &self.x {
                // The most recent window last, so it ends up in front.
                for w in &app.windows {
                    x.activate(*w);
                }
                return Ok(());
            }
        }
        let info = Self::info(id).ok_or_else(|| "Kairos can\u{2019}t find it on this computer. It may have been removed.".to_string())?;
        let ctx = gtk::gdk::Display::default().map(|d| d.app_launch_context());
        info.launch(&[], ctx.as_ref()).map_err(|e| e.to_string())
    }

    pub fn open_files(id: &str, files: &[std::path::PathBuf]) -> Result<(), String> {
        let info = Self::info(id).ok_or("app not found")?;
        let files: Vec<gio::File> = files.iter().map(gio::File::for_path).collect();
        let ctx = gtk::gdk::Display::default().map(|d| d.app_launch_context());
        info.launch(&files, ctx.as_ref()).map_err(|e| e.to_string())
    }

    pub fn window_title(&self, w: u32) -> String {
        self.x.as_ref().map(|x| x.title(w)).unwrap_or_default()
    }
}

pub fn stem(id: &str) -> String {
    id.trim_end_matches(".desktop").to_string()
}

/// Downloads and Trash on Kairos Bar.
pub mod folders {
    use gtk::gio;
    use gtk::glib;
    use gtk::prelude::*;
    use kairos_core::dock::DockFolder;

    pub fn uri(folder: DockFolder) -> String {
        match folder {
            DockFolder::Trash => "trash:///".into(),
            DockFolder::Downloads => {
                let dir = glib::user_special_dir(glib::UserDirectory::Downloads).unwrap_or_else(|| glib::home_dir().join("Downloads"));
                gio::File::for_path(dir).uri().to_string()
            }
        }
    }

    pub fn icon_name(folder: DockFolder, trash_full: bool) -> &'static str {
        match folder {
            DockFolder::Downloads => "folder-download",
            DockFolder::Trash => {
                if trash_full {
                    "user-trash-full"
                } else {
                    "user-trash"
                }
            }
        }
    }

    pub fn trash_is_full() -> bool {
        let file = gio::File::for_uri("trash:///");
        file.query_info("trash::item-count", gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)
            .map(|i| i.attribute_uint32("trash::item-count") > 0)
            .unwrap_or(false)
    }

    pub fn open(folder: DockFolder) {
        let ctx = gtk::gdk::Display::default().map(|d| d.app_launch_context());
        let _ = gio::AppInfo::launch_default_for_uri(&uri(folder), ctx.as_ref());
    }

    pub fn trash(paths: &[std::path::PathBuf]) {
        for p in paths {
            let _ = gio::File::for_path(p).trash(gio::Cancellable::NONE);
        }
    }
}
