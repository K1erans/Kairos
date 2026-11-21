//! The X11 side of Kairos: reading and changing other apps' windows through EWMH (which Muffin,
//! Cinnamon's window manager, follows), Kairos Bar's dock properties and struts, global
//! hotkeys, and the pointer. One connection, shared: a thread waits for events on it while the
//! main thread makes requests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::CURRENT_TIME;

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        _NET_CLIENT_LIST,
        _NET_ACTIVE_WINDOW,
        _NET_CURRENT_DESKTOP,
        _NET_NUMBER_OF_DESKTOPS,
        _NET_WM_DESKTOP,
        _NET_WM_PID,
        _NET_WM_NAME,
        _NET_WM_STATE,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_STICKY,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_SKIP_PAGER,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_WM_WINDOW_TYPE_DOCK,
        _NET_WM_WINDOW_TYPE_DESKTOP,
        _NET_WM_STRUT,
        _NET_WM_STRUT_PARTIAL,
        _NET_WM_USER_TIME,
        _NET_CLOSE_WINDOW,
        _NET_MOVERESIZE_WINDOW,
        _NET_FRAME_EXTENTS,
        _NET_WM_ALLOWED_ACTIONS,
        _NET_WM_ACTION_MOVE,
        _NET_WM_ACTION_RESIZE,
        _GTK_APPLICATION_ID,
        WM_CHANGE_STATE,
        UTF8_STRING,
    }
}

/// A window's place on the root window, frame included.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    /// The frame's left, right, top and bottom widths.
    pub frame: [u32; 4],
}

impl Geometry {
    pub fn outer(&self) -> (f64, f64, f64, f64) {
        let [l, r, t, b] = self.frame;
        (
            (self.x - l as i32) as f64,
            (self.y - t as i32) as f64,
            (self.w + l + r) as f64,
            (self.h + t + b) as f64,
        )
    }
}

#[derive(Clone, Debug)]
pub enum WmEvent {
    ActiveChanged,
    ClientsChanged,
    /// A window's state (minimised, full screen) or title changed.
    WindowChanged(u32),
    DesktopChanged,
    Hotkey(usize),
}

pub struct X11 {
    pub conn: Arc<RustConnection>,
    pub root: Window,
    pub atoms: Atoms,
    hotkeys: Mutex<Vec<(u8, u16)>>,
}

const LOCK: u16 = 1 << 1;
const NUM_LOCK: u16 = 1 << 4;
const MODIFIERS: u16 = 1 | (1 << 2) | (1 << 3) | (1 << 6);

impl X11 {
    pub fn connect() -> Option<Arc<X11>> {
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms::new(&conn).ok()?.reply().ok()?;
        let x = Arc::new(X11 { conn: Arc::new(conn), root, atoms, hotkeys: Mutex::new(vec![]) });
        x.conn
            .change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE))
            .ok()?;
        x.conn.flush().ok()?;
        Some(x)
    }

    /// Reads events on a thread and hands each to `send`. Client windows are watched for state
    /// and title changes as they appear.
    pub fn watch(self: &Arc<Self>, send: impl Fn(WmEvent) + Send + 'static) {
        let x = self.clone();
        for w in x.client_list() {
            x.select_client(w);
        }
        let _ = x.conn.flush();
        std::thread::spawn(move || loop {
            let Ok(event) = x.conn.wait_for_event() else { return };
            match event {
                Event::PropertyNotify(e) => {
                    if e.window == x.root {
                        if e.atom == x.atoms._NET_ACTIVE_WINDOW {
                            send(WmEvent::ActiveChanged);
                        } else if e.atom == x.atoms._NET_CLIENT_LIST {
                            for w in x.client_list() {
                                x.select_client(w);
                            }
                            let _ = x.conn.flush();
                            send(WmEvent::ClientsChanged);
                        } else if e.atom == x.atoms._NET_CURRENT_DESKTOP {
                            send(WmEvent::DesktopChanged);
                        }
                    } else if e.atom == x.atoms._NET_WM_STATE || e.atom == x.atoms._NET_WM_NAME {
                        send(WmEvent::WindowChanged(e.window));
                    }
                }
                Event::KeyPress(e) => {
                    let state = u16::from(e.state) & MODIFIERS;
                    let keys = x.hotkeys.lock().unwrap().clone();
                    if let Some(i) = keys.iter().position(|(code, mods)| *code == e.detail && *mods == state) {
                        send(WmEvent::Hotkey(i));
                    }
                }
                _ => {}
            }
        });
    }

    fn select_client(&self, w: Window) {
        let _ = self.conn.change_window_attributes(w, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE));
    }

    fn prop(&self, w: Window, atom: Atom) -> Option<GetPropertyReply> {
        self.conn.get_property(false, w, atom, AtomEnum::ANY, 0, 4096).ok()?.reply().ok()
    }

    fn prop32(&self, w: Window, atom: Atom) -> Vec<u32> {
        self.prop(w, atom).and_then(|r| r.value32().map(|v| v.collect())).unwrap_or_default()
    }

    fn prop_string(&self, w: Window, atom: Atom) -> Option<String> {
        let r = self.prop(w, atom)?;
        if r.value.is_empty() {
            return None;
        }
        Some(String::from_utf8_lossy(&r.value).trim_end_matches('\0').to_string())
    }

    pub fn client_list(&self) -> Vec<Window> {
        self.prop32(self.root, self.atoms._NET_CLIENT_LIST)
    }

    pub fn active_window(&self) -> Option<Window> {
        self.prop32(self.root, self.atoms._NET_ACTIVE_WINDOW).first().copied().filter(|w| *w != 0)
    }

    pub fn pid(&self, w: Window) -> Option<u32> {
        self.prop32(w, self.atoms._NET_WM_PID).first().copied()
    }

    pub fn wm_class(&self, w: Window) -> (String, String) {
        let raw = self.prop(w, AtomEnum::WM_CLASS.into()).map(|r| r.value).unwrap_or_default();
        let mut parts = raw.split(|b| *b == 0).map(|p| String::from_utf8_lossy(p).to_string());
        (parts.next().unwrap_or_default(), parts.next().unwrap_or_default())
    }

    pub fn gtk_app_id(&self, w: Window) -> Option<String> {
        self.prop_string(w, self.atoms._GTK_APPLICATION_ID)
    }

    pub fn title(&self, w: Window) -> String {
        self.prop_string(w, self.atoms._NET_WM_NAME).or_else(|| self.prop_string(w, AtomEnum::WM_NAME.into())).unwrap_or_default()
    }

    pub fn states(&self, w: Window) -> Vec<Atom> {
        self.prop32(w, self.atoms._NET_WM_STATE)
    }

    pub fn is_hidden(&self, w: Window) -> bool {
        self.states(w).contains(&self.atoms._NET_WM_STATE_HIDDEN)
    }

    pub fn is_full_screen(&self, w: Window) -> bool {
        self.states(w).contains(&self.atoms._NET_WM_STATE_FULLSCREEN)
    }

    /// A window that shows in a taskbar: normal or dialog type and not skipping the taskbar.
    pub fn is_app_window(&self, w: Window) -> bool {
        let types = self.prop32(w, self.atoms._NET_WM_WINDOW_TYPE);
        let a = &self.atoms;
        let ok_type = types.is_empty() || types.contains(&a._NET_WM_WINDOW_TYPE_NORMAL) || types.contains(&a._NET_WM_WINDOW_TYPE_DIALOG);
        ok_type && !self.states(w).contains(&a._NET_WM_STATE_SKIP_TASKBAR)
    }

    pub fn can(&self, w: Window, action: Atom) -> bool {
        let actions = self.prop32(w, self.atoms._NET_WM_ALLOWED_ACTIONS);
        actions.is_empty() || actions.contains(&action)
    }

    pub fn current_desktop(&self) -> u32 {
        self.prop32(self.root, self.atoms._NET_CURRENT_DESKTOP).first().copied().unwrap_or(0)
    }

    pub fn desktop_count(&self) -> u32 {
        self.prop32(self.root, self.atoms._NET_NUMBER_OF_DESKTOPS).first().copied().unwrap_or(1)
    }

    fn client_message(&self, w: Window, atom: Atom, data: [u32; 5]) {
        let event = ClientMessageEvent::new(32, w, atom, data);
        let _ = self.conn.send_event(false, self.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event);
        let _ = self.conn.flush();
    }

    /// Switches to workspace `number` (1-based).
    pub fn switch_desktop(&self, number: u32) -> bool {
        if number == 0 || number > self.desktop_count() {
            return false;
        }
        self.client_message(self.root, self.atoms._NET_CURRENT_DESKTOP, [number - 1, CURRENT_TIME, 0, 0, 0]);
        true
    }

    pub fn minimize(&self, w: Window) {
        self.client_message(w, self.atoms.WM_CHANGE_STATE, [3, 0, 0, 0, 0]);
    }

    /// Brings a window forward and restores it if minimised. Source 2 is a pager, which Muffin
    /// obeys without focus-stealing prevention.
    pub fn activate(&self, w: Window) {
        self.client_message(w, self.atoms._NET_ACTIVE_WINDOW, [2, CURRENT_TIME, 0, 0, 0]);
    }

    /// The workspace a window is on (0-based), or None when it is on all of them.
    fn desktop_of(&self, w: Window) -> Option<u32> {
        self.prop32(w, self.atoms._NET_WM_DESKTOP).first().copied().filter(|d| *d != 0xFFFF_FFFF)
    }

    /// Brings an app's windows forward as a Dock click does: those on the workspace on screen,
    /// or if none are there, the last window's workspace is switched to and its windows there
    /// come forward. Asked to activate a window on another workspace, Muffin only marks it as
    /// wanting attention, which shows nowhere while Kairos hides Cinnamon's panel.
    pub fn bring_forward(&self, windows: &[Window]) {
        let current = self.current_desktop();
        let mut chosen: Vec<Window> = windows.iter().copied().filter(|w| self.desktop_of(*w).map_or(true, |d| d == current)).collect();
        if chosen.is_empty() {
            let Some(desktop) = windows.last().and_then(|w| self.desktop_of(*w)) else { return };
            self.client_message(self.root, self.atoms._NET_CURRENT_DESKTOP, [desktop, CURRENT_TIME, 0, 0, 0]);
            chosen = windows.iter().copied().filter(|w| self.desktop_of(*w) == Some(desktop)).collect();
        }
        // The most recent window last, so it ends up in front.
        for w in chosen {
            self.activate(w);
        }
    }

    /// Moves a window to the workspace on screen, as a pager would.
    pub fn move_to_current_desktop(&self, w: Window) {
        self.client_message(w, self.atoms._NET_WM_DESKTOP, [self.current_desktop(), 2, 0, 0, 0]);
    }

    pub fn close(&self, w: Window) {
        self.client_message(w, self.atoms._NET_CLOSE_WINDOW, [CURRENT_TIME, 2, 0, 0, 0]);
    }

    pub fn geometry(&self, w: Window) -> Option<Geometry> {
        let g = self.conn.get_geometry(w).ok()?.reply().ok()?;
        let t = self.conn.translate_coordinates(w, self.root, 0, 0).ok()?.reply().ok()?;
        let extents = self.prop32(w, self.atoms._NET_FRAME_EXTENTS);
        let frame = if extents.len() == 4 { [extents[0], extents[1], extents[2], extents[3]] } else { [0; 4] };
        Some(Geometry { x: t.dst_x as i32, y: t.dst_y as i32, w: g.width as u32, h: g.height as u32, frame })
    }

    /// Moves and resizes a window's frame to (x, y, w, h).
    pub fn move_resize(&self, w: Window, frame: [u32; 4], x: f64, y: f64, width: f64, height: f64) {
        let [l, r, t, b] = frame;
        let inner_w = (width - (l + r) as f64).max(1.0) as u32;
        let inner_h = (height - (t + b) as f64).max(1.0) as u32;
        // NorthWest gravity with x, y, width and height set, from a pager.
        let flags = 1 | (0xf << 8) | (2 << 12);
        self.client_message(w, self.atoms._NET_MOVERESIZE_WINDOW, [flags, x as i32 as u32, y as i32 as u32, inner_w, inner_h]);
    }

    pub fn query_pointer(&self) -> Option<(f64, f64, bool)> {
        let r = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
        let button = u16::from(r.mask) & u16::from(KeyButMask::BUTTON1) != 0;
        Some((r.root_x as f64, r.root_y as f64, button))
    }

    /// Kairos Bar's properties, set before the window is mapped: a dock on every workspace.
    pub fn make_dock(&self, w: Window) {
        let a = &self.atoms;
        let _ = self.conn.change_property32(PropMode::REPLACE, w, a._NET_WM_WINDOW_TYPE, AtomEnum::ATOM, &[a._NET_WM_WINDOW_TYPE_DOCK]);
        let _ = self.conn.change_property32(
            PropMode::REPLACE,
            w,
            a._NET_WM_STATE,
            AtomEnum::ATOM,
            &[a._NET_WM_STATE_ABOVE, a._NET_WM_STATE_STICKY, a._NET_WM_STATE_SKIP_TASKBAR, a._NET_WM_STATE_SKIP_PAGER],
        );
        let _ = self.conn.change_property32(PropMode::REPLACE, w, a._NET_WM_DESKTOP, AtomEnum::CARDINAL, &[0xFFFF_FFFF]);
        let _ = self.conn.flush();
    }

    /// An overlay above everything, on every workspace, that does not take the keyboard when
    /// `quiet` (a break reminder that may arrive while the owner types).
    pub fn make_overlay(&self, w: Window, quiet: bool) {
        let a = &self.atoms;
        let _ = self.conn.change_property32(
            PropMode::REPLACE,
            w,
            a._NET_WM_STATE,
            AtomEnum::ATOM,
            &[a._NET_WM_STATE_ABOVE, a._NET_WM_STATE_STICKY, a._NET_WM_STATE_SKIP_TASKBAR, a._NET_WM_STATE_SKIP_PAGER, a._NET_WM_STATE_FULLSCREEN],
        );
        if quiet {
            let _ = self.conn.change_property32(PropMode::REPLACE, w, a._NET_WM_USER_TIME, AtomEnum::CARDINAL, &[0]);
        } else {
            let _ = self.conn.delete_property(w, a._NET_WM_USER_TIME);
        }
        let _ = self.conn.flush();
    }

    /// Keeps a window above others (after it is mapped).
    pub fn keep_above(&self, w: Window) {
        let a = &self.atoms;
        self.client_message(w, a._NET_WM_STATE, [1, a._NET_WM_STATE_ABOVE, a._NET_WM_STATE_STICKY, 2, 0]);
    }

    /// Reserves a strip of the screen edge, so maximised windows stop at Kairos Bar. `strut` is
    /// left, right, top, bottom and the start and end of each, in root coordinates.
    pub fn set_strut(&self, w: Window, strut: Option<[u32; 12]>) {
        let a = &self.atoms;
        match strut {
            Some(s) => {
                let _ = self.conn.change_property32(PropMode::REPLACE, w, a._NET_WM_STRUT_PARTIAL, AtomEnum::CARDINAL, &s);
                let _ = self.conn.change_property32(PropMode::REPLACE, w, a._NET_WM_STRUT, AtomEnum::CARDINAL, &s[..4]);
            }
            None => {
                let _ = self.conn.delete_property(w, a._NET_WM_STRUT_PARTIAL);
                let _ = self.conn.delete_property(w, a._NET_WM_STRUT);
            }
        }
        let _ = self.conn.flush();
    }

    pub fn configure(&self, w: Window, x: i32, y: i32, width: u32, height: u32) {
        let _ = self.conn.configure_window(w, &ConfigureWindowAux::new().x(x).y(y).width(width.max(1)).height(height.max(1)));
        let _ = self.conn.flush();
    }

    /// Struts other docks (Cinnamon's panels) reserve, as left, right, top and bottom.
    pub fn other_struts(&self, own: &[Window]) -> [u32; 4] {
        let mut out = [0u32; 4];
        for w in self.client_list().into_iter().chain(self.stacked_docks()) {
            if own.contains(&w) {
                continue;
            }
            let s = self.prop32(w, self.atoms._NET_WM_STRUT_PARTIAL);
            let s = if s.len() >= 4 { s } else { self.prop32(w, self.atoms._NET_WM_STRUT) };
            for i in 0..4.min(s.len()) {
                out[i] = out[i].max(s[i]);
            }
        }
        out
    }

    /// Dock windows are often left out of _NET_CLIENT_LIST, so look at the root's children too.
    fn stacked_docks(&self) -> Vec<Window> {
        let Ok(Ok(tree)) = self.conn.query_tree(self.root).map(|c| c.reply()) else { return vec![] };
        tree.children
            .into_iter()
            .filter(|w| self.prop32(*w, self.atoms._NET_WM_WINDOW_TYPE).contains(&self.atoms._NET_WM_WINDOW_TYPE_DOCK))
            .collect()
    }

    // MARK: Hotkeys

    fn keycodes(&self) -> HashMap<u32, u8> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let mut map = HashMap::new();
        let Ok(Ok(reply)) = self.conn.get_keyboard_mapping(min, max - min + 1).map(|c| c.reply()) else { return map };
        let per = reply.keysyms_per_keycode as usize;
        for (i, chunk) in reply.keysyms.chunks(per.max(1)).enumerate() {
            for sym in chunk {
                map.entry(*sym).or_insert(min + i as u8);
            }
        }
        map
    }

    /// Grabs the shortcuts; returns the indexes that could not be grabbed (taken by another app).
    pub fn grab_hotkeys(&self, keys: &[(u32, u16)]) -> Vec<usize> {
        let mut held = self.hotkeys.lock().unwrap();
        for (code, mods) in held.iter() {
            for extra in [0, LOCK, NUM_LOCK, LOCK | NUM_LOCK] {
                let _ = self.conn.ungrab_key(*code, self.root, ModMask::from(mods | extra));
            }
        }
        held.clear();
        let codes = self.keycodes();
        let mut failed = vec![];
        for (i, (sym, mods)) in keys.iter().enumerate() {
            let Some(code) = codes.get(sym).copied() else {
                failed.push(i);
                held.push((0, 0));
                continue;
            };
            let mut ok = true;
            for extra in [0, LOCK, NUM_LOCK, LOCK | NUM_LOCK] {
                let res = self
                    .conn
                    .grab_key(true, self.root, ModMask::from(mods | extra), code, GrabMode::ASYNC, GrabMode::ASYNC)
                    .map(|c| c.check());
                if !matches!(res, Ok(Ok(()))) {
                    ok = false;
                }
            }
            if !ok {
                for extra in [0, LOCK, NUM_LOCK, LOCK | NUM_LOCK] {
                    let _ = self.conn.ungrab_key(code, self.root, ModMask::from(mods | extra));
                }
                failed.push(i);
                held.push((0, 0));
            } else {
                held.push((code, *mods));
            }
        }
        let _ = self.conn.flush();
        failed
    }
}
