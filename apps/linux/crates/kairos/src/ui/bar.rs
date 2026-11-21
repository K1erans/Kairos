//! Kairos Bar: groups, then the active group's apps, then open apps and folders, along one
//! screen edge. Drawn with cairo in one transparent dock window; the pointer is followed by
//! polling X11, so magnification and auto-hide work even where the window takes no input.

use super::icons;
use super::widgets::actions;
use crate::model::{change, Model};
use crate::platform::apps::{folders, Apps};
use crate::platform::x11::X11;
use gtk::prelude::*;
use x11rb::connection::Connection;
use gtk::{gdk, gio, glib, pango};
use kairos_core::clearance::{self, Rect};
use kairos_core::dock::{self, DockAutoHide, DockDrop, DockFrame, DockLaunches, HideInputs, HideRule, Layout, Slot, INSET};
use kairos_core::*;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const DRAG_START: f64 = 6.0;
const DRAG_OFF: f64 = 70.0;
/// Resting on the edge this long over a full-screen app counts as the Mac's second push.
const FULL_SCREEN_DWELL: f64 = 0.8;

#[derive(Clone, Debug)]
enum Press {
    Slot { slot: Slot, x: f64, y: f64, dragging: bool },
    Divider { icon: f64, cross: f64 },
}

pub struct KairosBar {
    model: Rc<Model>,
    x: Arc<X11>,
    window: gtk::Window,
    area: gtk::DrawingArea,
    frame: RefCell<DockFrame>,
    xid: Cell<u32>,
    origin: Cell<(f64, f64)>,
    monitor: Cell<Rect>,
    clock: Instant,
    hide: RefCell<DockAutoHide>,
    pointer: Cell<Option<(f64, f64)>>,
    engaged: Cell<bool>,
    amount: Cell<f64>,
    last_step: Cell<f64>,
    edge_since: Cell<Option<f64>>,
    press: RefCell<Option<Press>>,
    menu_open: Cell<bool>,
    drop_hover: Cell<Option<(f64, f64)>>,
    launches: RefCell<DockLaunches>,
    trash_full: Cell<bool>,
    full_screen: Cell<bool>,
    timer: RefCell<Option<glib::SourceId>>,
    strut: Cell<Option<[u32; 12]>>,
    input: RefCell<Option<(i32, i32, i32, i32)>>,
}

fn now_s(clock: &Instant) -> f64 {
    clock.elapsed().as_secs_f64()
}

fn reduced_motion() -> bool {
    gtk::Settings::default().map(|s| !s.is_gtk_enable_animations()).unwrap_or(false)
}

impl KairosBar {
    pub fn new(app: &gtk::Application, model: &Rc<Model>, x: Arc<X11>) -> Rc<KairosBar> {
        let window = gtk::Window::builder().application(app).title("Kairos Bar").decorated(false).resizable(false).build();
        window.add_css_class("kairos-transparent");
        window.set_widget_name("kairos-bar");
        let area = gtk::DrawingArea::new();
        window.set_child(Some(&area));
        let frame = Self::frame_for(model);
        let rule = Self::rule(model, false);
        let clock = Instant::now();
        let me = Rc::new(KairosBar {
            model: model.clone(),
            x,
            window,
            area,
            frame: RefCell::new(frame),
            xid: Cell::new(0),
            origin: Cell::new((0.0, 0.0)),
            monitor: Cell::new(Rect::default()),
            hide: RefCell::new(DockAutoHide::new(rule, 0.0)),
            clock,
            pointer: Cell::new(None),
            engaged: Cell::new(false),
            amount: Cell::new(0.0),
            last_step: Cell::new(0.0),
            edge_since: Cell::new(None),
            press: RefCell::new(None),
            menu_open: Cell::new(false),
            drop_hover: Cell::new(None),
            launches: RefCell::new(DockLaunches::default()),
            trash_full: Cell::new(folders::trash_is_full()),
            full_screen: Cell::new(false),
            timer: RefCell::new(None),
            strut: Cell::new(None),
            input: RefCell::new(None),
        });
        {
            let weak = Rc::downgrade(&me);
            me.window.connect_realize(move |w| {
                let Some(me) = weak.upgrade() else { return };
                if let Some(xid) = w.surface().and_then(|s| s.downcast::<gdk4_x11::X11Surface>().ok()).map(|s| s.xid() as u32) {
                    me.xid.set(xid);
                    me.x.make_dock(xid);
                }
            });
        }
        {
            let weak = Rc::downgrade(&me);
            me.window.connect_map(move |_| {
                if let Some(me) = weak.upgrade() {
                    me.place();
                    me.update_input();
                }
            });
        }
        {
            let weak = Rc::downgrade(&me);
            me.area.set_draw_func(move |_, cr, _, _| {
                if let Some(me) = weak.upgrade() {
                    me.draw(cr);
                }
            });
        }
        me.wire_input();
        {
            let weak = Rc::downgrade(&me);
            model.subscribe(move |what| {
                let Some(me) = weak.upgrade() else { return };
                if what & (change::DATA | change::RUNNING) != 0 {
                    me.refresh();
                }
                if what & change::DATA != 0 {
                    me.area.queue_draw();
                }
            });
        }
        me.refresh();
        me.schedule(0);
        me
    }

    fn frame_for(model: &Model) -> DockFrame {
        let s = model.s();
        let slots = dock::DockGeometry::slots_for(&s.data, &s.open_apps);
        let names: Vec<String> = slots.iter().filter_map(|slot| slot_name(model, &s.data, slot)).collect();
        let widest = names.iter().map(|n| n.chars().count() as f64 * 7.5 + 24.0).fold(0.0, f64::max).min(320.0);
        DockFrame::new(slots, s.data.settings.icon_size.clamp(20.0, 64.0), s.data.settings.dock_edge, reduced_motion(), widest)
    }

    fn rule(model: &Model, full_screen: bool) -> HideRule {
        let st = &model.s().data.settings;
        HideRule::new(st.auto_hide_bar, st.hides_in_full_screen, full_screen)
    }

    /// Rebuilds the layout from the model and shows, hides or moves the window to match.
    pub fn refresh(self: &Rc<Self>) {
        let visible = self.model.s().data.settings.dock_visible;
        if !visible {
            if self.window.is_visible() {
                self.window.set_visible(false);
            }
            return;
        }
        let next = Self::frame_for(&self.model);
        let changed = *self.frame.borrow() != next;
        if changed {
            *self.frame.borrow_mut() = next;
        }
        self.trash_full.set(folders::trash_is_full());
        let ids: HashSet<String> = self.model.s().running_ids.clone();
        self.launches.borrow_mut().update(&ids, now_s(&self.clock));
        if !self.window.is_visible() {
            let (w, h) = self.frame.borrow().size();
            self.window.set_default_size(w as i32, h as i32);
            self.area.set_content_width(w as i32);
            self.area.set_content_height(h as i32);
            self.window.present();
        } else if changed {
            self.place();
        }
        self.update_input();
        self.update_strut();
        self.area.queue_draw();
        self.schedule(0);
    }

    pub fn active_window_changed(self: &Rc<Self>) {
        let fs = self.x.active_window().map(|w| self.x.is_full_screen(w) && self.x.pid(w) != Some(std::process::id())).unwrap_or(false);
        if fs != self.full_screen.get() {
            self.full_screen.set(fs);
            self.step();
        }
    }

    fn scale(&self) -> f64 {
        self.window.surface().map(|s| s.scale_factor() as f64).unwrap_or(1.0)
    }

    fn work_area(&self, monitor: Rect) -> Rect {
        if self.model.s().data.settings.replaces_system_dock {
            return monitor;
        }
        let own = [self.xid.get()];
        let [l, r, t, b] = self.x.other_struts(&own);
        let scale = self.scale();
        let root = self.root_size();
        let left = (l as f64 / scale - monitor.x).max(0.0);
        let top = (t as f64 / scale - monitor.y).max(0.0);
        let right = (r as f64 / scale - (root.0 - monitor.max_x())).max(0.0);
        let bottom = (b as f64 / scale - (root.1 - monitor.max_y())).max(0.0);
        Rect::new(monitor.x + left, monitor.y + top, (monitor.w - left - right).max(100.0), (monitor.h - top - bottom).max(100.0))
    }

    fn root_size(&self) -> (f64, f64) {
        let screen = &self.x.conn.setup().roots[0];
        let scale = self.scale();
        (screen.width_in_pixels as f64 / scale, screen.height_in_pixels as f64 / scale)
    }

    fn primary_monitor(&self) -> Rect {
        let Some(display) = gdk::Display::default() else { return Rect::new(0.0, 0.0, 1920.0, 1080.0) };
        let list = display.monitors();
        let mut best: Option<Rect> = None;
        for i in 0..list.n_items() {
            if let Some(m) = list.item(i).and_then(|o| o.downcast::<gdk::Monitor>().ok()) {
                let g = m.geometry();
                let r = Rect::new(g.x() as f64, g.y() as f64, g.width() as f64, g.height() as f64);
                // The monitor at the origin is Cinnamon's primary by default.
                if best.is_none() || (r.x == 0.0 && r.y == 0.0) {
                    best = Some(r);
                }
            }
        }
        best.unwrap_or(Rect::new(0.0, 0.0, 1920.0, 1080.0))
    }

    /// Sizes the window to the frame and puts it on its edge.
    fn place(&self) {
        let xid = self.xid.get();
        if xid == 0 {
            return;
        }
        let frame = self.frame.borrow();
        let (w, h) = frame.size();
        self.area.set_content_width(w as i32);
        self.area.set_content_height(h as i32);
        self.window.set_default_size(w as i32, h as i32);
        let monitor = self.primary_monitor();
        self.monitor.set(monitor);
        let work = self.work_area(monitor);
        let mid = frame.lead + frame.geometry.rest_length() / 2.0;
        let (ox, oy) = dock::panel_origin(frame.edge, w, h, mid, monitor, work);
        // The bar sits on the screen edge itself, not the work area's, on the edge it uses.
        let (ox, oy) = match frame.edge {
            DockEdge::Bottom => (ox, if self.model.s().data.settings.replaces_system_dock { monitor.max_y() - h } else { oy }),
            _ => (ox, oy),
        };
        self.origin.set((ox, oy));
        let s = self.scale();
        self.x.configure(xid, (ox * s) as i32, (oy * s) as i32, (w * s) as u32, (h * s) as u32);
    }

    // MARK: Pointer and motion

    /// Polls the pointer and moves the bar on. Runs often while something moves, rarely when idle.
    fn step(self: &Rc<Self>) {
        if !self.window.is_visible() {
            return;
        }
        let now = now_s(&self.clock);
        let dt = (now - self.last_step.get()).clamp(0.0, 0.1);
        self.last_step.set(now);
        let frame = self.frame.borrow().clone();
        let (ox, oy) = self.origin.get();
        let scale = self.scale();
        let (pointer, button) = match self.x.query_pointer() {
            Some((px, py, b)) => (Some((px / scale - ox, py / scale - oy)), b),
            None => (None, false),
        };
        let drop = self.drop_hover.get();
        let local = drop.or(pointer);
        let bar = local.map(|(lx, ly)| frame.bar(lx, ly));
        let hidden_away = !self.hide.borrow().is_out();
        let engaged = match bar {
            Some((axis, cross)) if !hidden_away || drop.is_some() => frame.geometry.engages(axis, cross - INSET, 0.0, self.engaged.get()) || drop.is_some(),
            _ => false,
        };
        let held = self.menu_open.get() || matches!(*self.press.borrow(), Some(Press::Slot { dragging: true, .. }) | Some(Press::Divider { .. })) || drop.is_some();
        let at_edge = bar.map(|(a, c)| frame.geometry.touches_edge(a, c)).unwrap_or(false);
        if at_edge {
            if self.edge_since.get().is_none() {
                self.edge_since.set(Some(now));
            }
        } else {
            self.edge_since.set(None);
        }
        let pushed = self.edge_since.get().map(|t| now - t >= FULL_SCREEN_DWELL).unwrap_or(false);
        let rule = Self::rule(&self.model, self.full_screen.get());
        let was_out = self.hide.borrow().is_out();
        self.hide.borrow_mut().update(HideInputs { at_edge, on_bar: engaged, held, pushed }, Some(rule), now);
        let out = self.hide.borrow().is_out();
        if engaged != self.engaged.get() {
            self.engaged.set(engaged);
        }
        // Ease the magnification in and out.
        let target = if engaged && !frame.motion.reduced { 1.0 } else { 0.0 };
        let a = self.amount.get();
        let k = 1.0 - (-dt / 0.07).exp();
        let mut next = a + (target - a) * k;
        if (next - target).abs() < 0.005 {
            next = target;
        }
        let moved = self.pointer.get() != local || next != a;
        self.amount.set(next);
        self.pointer.set(local);
        // Dragging an app: start once it moves far enough.
        if let Some(Press::Slot { x, y, dragging, slot }) = self.press.borrow_mut().as_mut() {
            if !*dragging && button {
                if let Some((lx, ly)) = local {
                    if (lx - *x).hypot(ly - *y) > DRAG_START && matches!(slot, Slot::App(_)) {
                        *dragging = true;
                    }
                }
            }
        }
        if let Some(Press::Divider { icon, cross }) = self.press.borrow().clone() {
            if let Some((_, c)) = bar {
                let size = ((icon + (c - cross) / frame.motion.max_scale()) / 2.0).round() * 2.0;
                let size = size.clamp(20.0, 64.0);
                if size != self.model.s().data.settings.icon_size {
                    self.model.edit(|d| d.settings.icon_size = size);
                }
            }
        }
        let sliding = self.hide.borrow().is_sliding();
        if moved || sliding || was_out != out {
            self.area.queue_draw();
        }
        if was_out != out || moved {
            self.update_input();
        }
        if was_out != out {
            self.update_strut();
        }
        let busy = engaged || next != target || sliding || at_edge || held || !self.launches.borrow().is_empty() || self.press.borrow().is_some();
        let wait = if busy {
            8
        } else {
            match self.hide.borrow().next_decision() {
                Some(t) => (((t - now) * 1000.0) as u64).clamp(8, 120),
                None => 120,
            }
        };
        self.schedule(wait);
    }

    fn schedule(self: &Rc<Self>, ms: u64) {
        if let Some(id) = self.timer.borrow_mut().take() {
            id.remove();
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_millis(ms), move || {
            if let Some(me) = weak.upgrade() {
                me.timer.borrow_mut().take();
                me.step();
            }
        });
        *self.timer.borrow_mut() = Some(id);
    }

    fn layout(&self) -> Layout {
        let frame = self.frame.borrow();
        let axis = self.pointer.get().map(|(x, y)| frame.bar(x, y).0);
        if self.amount.get() <= 0.0 {
            return frame.geometry.layout(None, 0.0);
        }
        let mut l = frame.geometry.layout(axis, self.amount.get());
        if !self.engaged.get() {
            l.hovered = None;
        }
        l
    }

    /// How far the bar is pushed toward the screen edge by auto-hide, and its opacity.
    fn slide(&self) -> (f64, f64) {
        let frame = self.frame.borrow();
        let reveal = self.hide.borrow().visible(now_s(&self.clock));
        let (away, opacity) = frame.motion.appearance(reveal);
        (away * (INSET + frame.geometry.thickness() + frame.geometry.headroom() + 2.0), opacity)
    }

    /// Only the bar takes clicks; the rest of the window lets them through to what is below.
    fn update_input(&self) {
        let Some(surface) = self.window.surface() else { return };
        let frame = self.frame.borrow();
        let (offset, _) = self.slide();
        let rect = if !self.hide.borrow().is_out() && self.drop_hover.get().is_none() {
            (0, 0, 0, 0)
        } else {
            let l = self.layout();
            let thick = frame.geometry.thickness() + if self.engaged.get() { frame.geometry.headroom() } else { 0.0 };
            let (start, end) = (l.start.min(0.0), l.end.max(frame.geometry.rest_length()));
            let (x0, y0) = frame.point(start, -offset);
            let (x1, y1) = frame.point(end, INSET + thick - offset);
            let (lx, ly) = (x0.min(x1).floor() as i32, y0.min(y1).floor() as i32);
            (lx, ly, ((x0 - x1).abs().ceil() as i32).max(1), ((y0 - y1).abs().ceil() as i32).max(1))
        };
        if self.input.borrow().as_ref() == Some(&rect) {
            return;
        }
        *self.input.borrow_mut() = Some(rect);
        let region = gtk::cairo::Region::create_rectangle(&gtk::cairo::RectangleInt::new(rect.0, rect.1, rect.2, rect.3));
        surface.set_input_region(&region);
    }

    /// While pinned, reserves the bar's strip so maximised windows stop at it.
    fn update_strut(&self) {
        let xid = self.xid.get();
        if xid == 0 {
            return;
        }
        let st = self.model.s().data.settings.clone();
        let rule = Self::rule(&self.model, false);
        let frame = self.frame.borrow();
        let wanted = if rule == HideRule::Pinned && st.keeps_windows_clear && st.dock_visible {
            let s = self.scale();
            let depth = ((INSET + frame.geometry.thickness()) * s).ceil() as u32;
            let (ox, oy) = self.origin.get();
            let root = self.root_size();
            let m = self.monitor.get();
            let start = |v: f64| (v * s).max(0.0) as u32;
            let mut strut = [0u32; 12];
            match frame.edge {
                DockEdge::Bottom => {
                    strut[3] = depth + ((root.1 - m.max_y()) * s) as u32;
                    strut[10] = start(ox + frame.lead);
                    strut[11] = start(ox + frame.lead + frame.geometry.rest_length());
                }
                DockEdge::Left => {
                    strut[0] = depth + (m.x * s) as u32;
                    strut[4] = start(oy + frame.lead);
                    strut[5] = start(oy + frame.lead + frame.geometry.rest_length());
                }
                DockEdge::Right => {
                    strut[1] = depth + ((root.0 - m.max_x()) * s) as u32;
                    strut[6] = start(oy + frame.lead);
                    strut[7] = start(oy + frame.lead + frame.geometry.rest_length());
                }
            }
            Some(strut)
        } else {
            None
        };
        if self.strut.get() != wanted {
            self.strut.set(wanted);
            self.x.set_strut(xid, wanted);
        }
        drop(frame);
        if wanted.is_some() {
            self.clear_windows();
        }
    }

    /// Moves or shortens windows that sit under the pinned bar.
    pub fn clear_windows(&self) {
        let st = self.model.s().data.settings.clone();
        if Self::rule(&self.model, false) != HideRule::Pinned || !st.keeps_windows_clear || !st.dock_visible || kairos_core::store::is_scratch() {
            return;
        }
        let frame = self.frame.borrow();
        let (ox, oy) = self.origin.get();
        let (w, h) = frame.size();
        let s = self.scale();
        let panel = Rect::new(ox * s, oy * s, w * s, h * s);
        let depth = (INSET + frame.geometry.thickness()) * s;
        let strip = clearance::strip(frame.edge, panel, frame.lead * s, frame.geometry.rest_length() * s, depth);
        let m = self.monitor.get();
        let area = Rect::new(m.x * s, m.y * s, m.w * s, m.h * s);
        for win in self.x.client_list() {
            if win == self.xid.get() || !self.x.is_app_window(win) {
                continue;
            }
            let Some(g) = self.x.geometry(win) else { continue };
            let (fx, fy, fw, fh) = g.outer();
            let mut cw = clearance::Window::new(Rect::new(fx, fy, fw, fh));
            cw.can_move = self.x.can(win, self.x.atoms._NET_WM_ACTION_MOVE);
            cw.can_resize = self.x.can(win, self.x.atoms._NET_WM_ACTION_RESIZE);
            cw.is_full_screen = self.x.is_full_screen(win);
            cw.is_minimized = self.x.is_hidden(win);
            if let Some(r) = clearance::clear(&cw, &strip, frame.edge, &area) {
                self.x.move_resize(win, g.frame, r.x, r.y, r.w, r.h);
            }
        }
    }

    // MARK: Drawing

    fn draw(&self, cr: &gtk::cairo::Context) {
        cr.set_operator(gtk::cairo::Operator::Source);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        let _ = cr.paint();
        cr.set_operator(gtk::cairo::Operator::Over);
        let frame = self.frame.borrow().clone();
        let (offset, opacity) = self.slide();
        if opacity <= 0.0 || offset >= INSET + frame.geometry.thickness() + frame.geometry.headroom() + 1.0 {
            return;
        }
        let s = self.model.s();
        let appearance = s.data.settings.bar_appearance;
        let dark = true;
        let glass = matches!(appearance, BarAppearance::Glass);
        let l = self.layout();
        let g = &frame.geometry;
        let pad = g.padding();
        let thick = g.thickness();
        let _ = cr.save();
        cr.push_group();
        // The bar.
        let (x0, y0) = frame.point(l.start, INSET - offset);
        let (x1, y1) = frame.point(l.end, INSET + thick - offset);
        let (bx, by, bw, bh) = (x0.min(x1), y0.min(y1), (x0 - x1).abs(), (y0 - y1).abs());
        rounded(cr, bx, by, bw, bh, g.corner_radius());
        if glass {
            cr.set_source_rgba(0.55, 0.57, 0.62, 0.38);
        } else {
            cr.set_source_rgba(0.12, 0.12, 0.14, 0.82);
        }
        let _ = cr.fill_preserve();
        cr.set_source_rgba(if dark { 1.0 } else { 0.0 }, if dark { 1.0 } else { 0.0 }, if dark { 1.0 } else { 0.0 }, 0.14);
        cr.set_line_width(1.0);
        let _ = cr.stroke();
        let active = s.data.focus.active_group_id;
        let launching = self.launches.borrow().ids();
        let dragging = match &*self.press.borrow() {
            Some(Press::Slot { slot, dragging: true, .. }) => Some(slot.clone()),
            _ => None,
        };
        let mut hovered_name: Option<(String, f64, f64)> = None;
        for (i, p) in l.items.iter().enumerate() {
            let size = if p.slot.magnifies() { g.icon * p.scale } else { g.icon };
            let base = INSET + pad - offset;
            let (cx, cy) = frame.point(p.center, base + size / 2.0);
            let ghost = dragging.as_ref() == Some(&p.slot);
            if ghost {
                let _ = cr.save();
                cr.push_group();
            }
            match &p.slot {
                Slot::Kairos => {
                    let px = (size * self.scale()).round() as i32;
                    if let Some(pb) = icons::named("kairos", px.max(16)) {
                        paint_pixbuf(cr, &pb, cx - size / 2.0, cy - size / 2.0, size, 1.0);
                    }
                }
                Slot::Group(id) => {
                    if let Some(group) = s.data.group(Some(*id)) {
                        let (r, gg, b) = icons::rgb(&group.color_hex);
                        let radius = size * 0.42;
                        cr.set_source_rgb(r, gg, b);
                        cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
                        let _ = cr.fill();
                        if Some(*id) == active {
                            cr.set_source_rgba(if dark { 1.0 } else { 0.1 }, if dark { 1.0 } else { 0.1 }, if dark { 1.0 } else { 0.1 }, 0.9);
                            cr.set_line_width((size * 0.06).max(1.5));
                            cr.arc(cx, cy, radius + size * 0.07, 0.0, std::f64::consts::TAU);
                            let _ = cr.stroke();
                        }
                        symbol(cr, icons::group_icon(&group.icon), cx, cy, size * 0.46, (1.0, 1.0, 1.0));
                    }
                }
                Slot::NewGroup => {
                    let radius = size * 0.42;
                    let c = if dark { 1.0 } else { 0.0 };
                    cr.set_source_rgba(c, c, c, 0.18);
                    cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
                    let _ = cr.fill();
                    plus(cr, cx, cy, size * 0.36, (c, c, c, 0.85));
                }
                Slot::Divider | Slot::Separator => {
                    let c = if dark { 1.0 } else { 0.0 };
                    cr.set_source_rgba(c, c, c, if matches!(p.slot, Slot::Divider) { 0.35 } else { 0.22 });
                    let (ax, ay) = frame.point(p.center, INSET + pad * 0.8 - offset);
                    let (bx2, by2) = frame.point(p.center, INSET + thick - pad * 0.8 - offset);
                    cr.set_line_width(if matches!(p.slot, Slot::Divider) { 2.0 } else { 1.0 });
                    cr.move_to(ax, ay);
                    cr.line_to(bx2, by2);
                    let _ = cr.stroke();
                }
                Slot::App(id) | Slot::Running(id) => {
                    let px = (size * self.scale()).round() as i32;
                    let alpha = if launching.contains(id) { 0.55 + 0.45 * (now_s(&self.clock) * 5.0).sin().abs() } else { 1.0 };
                    let blocked = s.mode().blocks() && s.is_app_blocked(id);
                    if let Some(pb) = icons::pixbuf(id, || Apps::icon(id), px.max(16)) {
                        paint_pixbuf(cr, &pb, cx - size / 2.0, cy - size / 2.0, size, if blocked { alpha * 0.4 } else { alpha });
                    }
                    if s.running_ids.contains(id) {
                        let (dx, dy) = frame.point(p.center, INSET + 3.0 - offset);
                        let c = if dark { 1.0 } else { 0.1 };
                        cr.set_source_rgba(c, c, c, 0.85);
                        cr.arc(dx, dy, 2.0, 0.0, std::f64::consts::TAU);
                        let _ = cr.fill();
                    }
                }
                Slot::AddApp => {
                    let c = if dark { 1.0 } else { 0.0 };
                    rounded(cr, cx - size * 0.42, cy - size * 0.42, size * 0.84, size * 0.84, size * 0.2);
                    cr.set_source_rgba(c, c, c, 0.5);
                    cr.set_dash(&[4.0, 3.0], 0.0);
                    cr.set_line_width(1.5);
                    let _ = cr.stroke();
                    cr.set_dash(&[], 0.0);
                    plus(cr, cx, cy, size * 0.3, (c, c, c, 0.8));
                }
                Slot::Folder(f) => {
                    let px = (size * self.scale()).round() as i32;
                    let name = folders::icon_name(*f, self.trash_full.get());
                    if let Some(pb) = icons::named(name, px.max(16)) {
                        paint_pixbuf(cr, &pb, cx - size / 2.0, cy - size / 2.0, size, 1.0);
                    }
                }
            }
            if ghost {
                let _ = cr.pop_group_to_source();
                let _ = cr.paint_with_alpha(0.35);
                let _ = cr.restore();
            }
            if l.hovered == Some(i) && self.press.borrow().is_none() && !self.menu_open.get() {
                if let Some(name) = slot_name(&self.model, &s.data, &p.slot) {
                    hovered_name = Some((name, p.center, base + size));
                }
            }
        }
        // The dragged app follows the pointer.
        if let (Some(Slot::App(id)), Some((lx, ly))) = (&dragging, self.pointer.get()) {
            let size = g.icon * g.max_scale.min(1.2);
            if let Some(pb) = icons::pixbuf(id, || Apps::icon(id), (size * self.scale()).round() as i32) {
                paint_pixbuf(cr, &pb, lx - size / 2.0, ly - size / 2.0, size, 0.9);
            }
        }
        if let Some((name, along, top)) = hovered_name {
            self.draw_label(cr, &frame, &l, &name, along, top + dock::label::GAP);
        }
        let _ = cr.pop_group_to_source();
        let _ = cr.paint_with_alpha(opacity);
        let _ = cr.restore();
    }

    fn draw_label(&self, cr: &gtk::cairo::Context, frame: &DockFrame, l: &Layout, name: &str, along: f64, across: f64) {
        let layout = pangocairo::functions::create_layout(cr);
        let mut font = pango::FontDescription::new();
        font.set_size(11 * pango::SCALE);
        font.set_weight(pango::Weight::Medium);
        layout.set_font_description(Some(&font));
        layout.set_text(name);
        let (tw, th) = layout.pixel_size();
        let (bw, bh) = (tw as f64 + 20.0, th as f64 + 10.0);
        let (fw, fh) = frame.size();
        let (x, y) = match frame.edge {
            DockEdge::Bottom => {
                let along_len = frame.axis_length();
                let c = dock::label::center(frame.lead + along, bw, 0.0, along_len.max(fw)) ;
                let (_, py) = frame.point(along, across);
                (c - bw / 2.0, py - bh)
            }
            DockEdge::Left => {
                let (px, py) = frame.point(along, across);
                (px, (py - bh / 2.0).clamp(0.0, fh - bh))
            }
            DockEdge::Right => {
                let (px, py) = frame.point(along, across);
                (px - bw, (py - bh / 2.0).clamp(0.0, fh - bh))
            }
        };
        let _ = l;
        rounded(cr, x, y, bw, bh, bh / 2.0);
        cr.set_source_rgba(0.1, 0.1, 0.12, 0.88);
        let _ = cr.fill();
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.move_to(x + 10.0, y + 5.0);
        pangocairo::functions::show_layout(cr, &layout);
    }

    // MARK: Input

    fn slot_at_local(&self, lx: f64, ly: f64) -> Option<Slot> {
        let frame = self.frame.borrow();
        let (axis, cross) = frame.bar(lx, ly);
        if cross < -2.0 || cross > INSET + frame.geometry.thickness() + frame.geometry.headroom() + 2.0 {
            return None;
        }
        let l = self.layout();
        // Dividers do not magnify, so they are found by plain distance.
        for p in &l.items {
            if matches!(p.slot, Slot::Divider) && (axis - p.center).abs() <= dock::DIVIDER_LENGTH / 2.0 + frame.geometry.gap() / 2.0 {
                return Some(Slot::Divider);
            }
        }
        let hit = l.items.iter().find(|p| p.slot.magnifies() && (axis - p.center).abs() <= p.size / 2.0 + frame.geometry.gap() / 2.0);
        hit.map(|p| p.slot.clone())
    }

    fn wire_input(self: &Rc<Self>) {
        let drag = gtk::GestureDrag::new();
        drag.set_button(0);
        {
            let weak = Rc::downgrade(self);
            drag.connect_drag_begin(move |g, x, y| {
                let Some(me) = weak.upgrade() else { return };
                let Some(slot) = me.slot_at_local(x, y) else {
                    g.set_state(gtk::EventSequenceState::Denied);
                    return;
                };
                if g.current_button() == 3 {
                    g.set_state(gtk::EventSequenceState::Claimed);
                    me.menu(&slot, x, y);
                    return;
                }
                if g.current_button() != 1 {
                    return;
                }
                *me.press.borrow_mut() = Some(if slot == Slot::Divider {
                    let cross = me.frame.borrow().bar(x, y).1;
                    Press::Divider { icon: me.model.s().data.settings.icon_size, cross }
                } else {
                    Press::Slot { slot, x, y, dragging: false }
                });
                me.schedule(0);
            });
        }
        {
            let weak = Rc::downgrade(self);
            drag.connect_drag_end(move |g, dx, dy| {
                let Some(me) = weak.upgrade() else { return };
                if g.current_button() != 1 {
                    return;
                }
                let press = me.press.borrow_mut().take();
                match press {
                    Some(Press::Slot { slot, x, y, dragging }) => {
                        if dragging {
                            me.finish_drag(&slot, x + dx, y + dy);
                        } else if dx.hypot(dy) <= DRAG_START {
                            me.click(&slot);
                        }
                    }
                    Some(Press::Divider { .. }) | None => {}
                }
                me.area.queue_draw();
                me.schedule(0);
            });
        }
        self.area.add_controller(drag);
        // Files dropped on the bar.
        let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY | gdk::DragAction::MOVE);
        {
            let weak = Rc::downgrade(self);
            target.connect_motion(move |_, x, y| {
                if let Some(me) = weak.upgrade() {
                    me.drop_hover.set(Some((x, y)));
                    me.schedule(0);
                }
                gdk::DragAction::COPY
            });
        }
        {
            let weak = Rc::downgrade(self);
            target.connect_leave(move |_| {
                if let Some(me) = weak.upgrade() {
                    me.drop_hover.set(None);
                    me.schedule(0);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            target.connect_drop(move |_, value, x, y| {
                let Some(me) = weak.upgrade() else { return false };
                me.drop_hover.set(None);
                let Ok(list) = value.get::<gdk::FileList>() else { return false };
                let paths: Vec<std::path::PathBuf> = list.files().iter().filter_map(|f| f.path()).collect();
                let slot = me.slot_at_local(x, y);
                me.dropped(paths, slot);
                true
            });
        }
        self.area.add_controller(target);
    }

    fn dropped(self: &Rc<Self>, paths: Vec<std::path::PathBuf>, slot: Option<Slot>) {
        match DockDrop::plan(&paths, slot.as_ref(), &|p| Apps::id_for_file(p)) {
            DockDrop::AddApps(ids) => {
                for id in ids {
                    let _ = self.model.add_dock_app(&id, None);
                }
            }
            DockDrop::Open(files, id) => {
                if let Err(e) = Apps::open_files(&id, &files) {
                    eprintln!("Kairos: opening files with {id} failed: {e}");
                }
            }
            DockDrop::MoveToTrash(paths) => {
                folders::trash(&paths);
                self.trash_full.set(true);
                self.area.queue_draw();
            }
            DockDrop::Nothing => {}
        }
    }

    fn finish_drag(self: &Rc<Self>, slot: &Slot, lx: f64, ly: f64) {
        let Slot::App(id) = slot else { return };
        let frame = self.frame.borrow().clone();
        let (axis, cross) = frame.bar(lx, ly);
        if cross > INSET + frame.geometry.thickness() + frame.geometry.headroom() + DRAG_OFF || cross < -DRAG_OFF {
            self.model.remove_dock_app(id);
            return;
        }
        let l = frame.geometry.layout(None, 0.0);
        let centers: Vec<f64> = l.items.iter().filter(|p| matches!(p.slot, Slot::App(_))).map(|p| p.center).collect();
        if let Some(i) = dock::reorder::index(&centers, axis) {
            self.model.move_dock_app_to(id, i);
        }
    }

    fn click(self: &Rc<Self>, slot: &Slot) {
        let m = self.model.clone();
        match slot.clone() {
            Slot::Kairos => glib::idle_add_local_once(move || show_main_window(&m)),
            Slot::Group(id) => glib::idle_add_local_once(move || m.switch_group(id)),
            Slot::NewGroup => glib::idle_add_local_once(move || actions::new_group(&m)),
            Slot::AddApp => glib::idle_add_local_once(move || actions::add_apps(&m)),
            Slot::App(id) | Slot::Running(id) => {
                let running = m.s().running.clone();
                if !running.iter().any(|a| a.id == id) {
                    self.launches.borrow_mut().start(&id, now_s(&self.clock));
                }
                if let Err(e) = m.apps.open(&id, &running) {
                    let name = m.apps.name(&id);
                    super::widgets::alert(None, &format!("Kairos couldn\u{2019}t open {name}"), &e);
                }
                self.schedule(0);
                return;
            }
            Slot::Folder(f) => {
                folders::open(f);
                return;
            }
            Slot::Divider | Slot::Separator => return,
        };
    }

    fn menu(self: &Rc<Self>, slot: &Slot, x: f64, y: f64) {
        let menu = gio::Menu::new();
        let group = gio::SimpleActionGroup::new();
        let m = self.model.clone();
        let add = |name: &str, f: Box<dyn Fn()>| {
            let a = gio::SimpleAction::new(name, None);
            a.connect_activate(move |_, _| f());
            group.add_action(&a);
        };
        match slot.clone() {
            Slot::Kairos => {
                menu.append(Some("Open Kairos"), Some("bar.open"));
                let mm = m.clone();
                add("open", Box::new(move || show_main_window(&mm)));
            }
            Slot::Group(id) => {
                let active = m.s().data.focus.active_group_id == Some(id);
                if !active {
                    menu.append(Some("Switch to This Group"), Some("bar.switch"));
                }
                menu.append(Some("Rename\u{2026}"), Some("bar.rename"));
                menu.append(Some("Edit Group\u{2026}"), Some("bar.edit"));
                if m.s().data.groups.len() > 1 {
                    menu.append(Some("Delete Group\u{2026}"), Some("bar.delete"));
                }
                let mm = m.clone();
                add("switch", Box::new(move || mm.switch_group(id)));
                let mm = m.clone();
                add("rename", Box::new(move || actions::rename(&mm, id)));
                let mm = m.clone();
                add("edit", Box::new(move || actions::edit_groups(&mm, Some(id))));
                let mm = m.clone();
                add("delete", Box::new(move || actions::delete(&mm, id)));
            }
            Slot::NewGroup => {
                menu.append(Some("New Group\u{2026}"), Some("bar.new"));
                let mm = m.clone();
                add("new", Box::new(move || actions::new_group(&mm)));
            }
            Slot::App(id) | Slot::Running(id) => {
                let running: Vec<_> = m.s().running.iter().filter(|a| a.id == id).cloned().collect();
                let pinned = matches!(slot, Slot::App(_));
                let section = gio::Menu::new();
                // The app's windows, by title.
                for (i, w) in running.iter().flat_map(|a| a.windows.clone()).enumerate().take(12) {
                    let title = m.apps.window_title(w);
                    let title = if title.is_empty() { m.apps.name(&id) } else { title };
                    let short: String = title.chars().take(60).collect();
                    section.append(Some(&short), Some(&format!("bar.window{i}")));
                    let x = m.x.clone();
                    add(&format!("window{i}"), Box::new(move || {
                        if let Some(x) = &x {
                            x.bring_forward(&[w]);
                        }
                    }));
                }
                if section.n_items() > 0 {
                    menu.append_section(None, &section);
                }
                let options = gio::Menu::new();
                let locked = m.s().active_group().map(|g| m.s().rule_lock(g.id).is_some()).unwrap_or(false);
                if pinned {
                    options.append(Some("Remove from Kairos Bar"), Some("bar.unpin"));
                    let (mm, id2) = (m.clone(), id.clone());
                    add("unpin", Box::new(move || mm.remove_dock_app(&id2)));
                    let apps = m.s().active_group().map(|g| g.dock_apps.clone()).unwrap_or_default();
                    let pos = apps.iter().position(|a| *a == id).unwrap_or(0);
                    if pos > 0 {
                        options.append(Some("Move Left"), Some("bar.left"));
                    }
                    if pos + 1 < apps.len() {
                        options.append(Some("Move Right"), Some("bar.right"));
                    }
                    let (mm, id2) = (m.clone(), id.clone());
                    add("left", Box::new(move || mm.move_dock_app_by(&id2, -1, None)));
                    let (mm, id2) = (m.clone(), id.clone());
                    add("right", Box::new(move || mm.move_dock_app_by(&id2, 1, None)));
                } else if !locked {
                    // Jev trusts the apps in the active group's bar, so pinning waits for the lock.
                    options.append(Some("Keep in Kairos Bar"), Some("bar.pin"));
                    let (mm, id2) = (m.clone(), id.clone());
                    add("pin", Box::new(move || {
                        mm.add_dock_app(&id2, None);
                    }));
                }
                let group_name = m.s().active_group().map(|g| g.name.clone()).unwrap_or_default();
                let blocked = m.s().active_group().map(|g| g.blocked_apps.contains(&id)).unwrap_or(false);
                if blocked {
                    if !locked {
                        options.append(Some(&format!("Unblock in {group_name}")), Some("bar.unblock"));
                    }
                } else {
                    options.append(Some(&format!("Block in {group_name}")), Some("bar.block"));
                }
                let (mm, id2) = (m.clone(), id.clone());
                add("block", Box::new(move || mm.set_app_blocked(&id2, true)));
                let (mm, id2) = (m.clone(), id.clone());
                add("unblock", Box::new(move || mm.set_app_blocked(&id2, false)));
                menu.append_section(None, &options);
                let last = gio::Menu::new();
                if running.is_empty() {
                    last.append(Some("Open"), Some("bar.open"));
                } else {
                    last.append(Some("New Window"), Some("bar.new_window"));
                    last.append(Some("Quit"), Some("bar.quit"));
                }
                let (mm, id2) = (m.clone(), id.clone());
                add("open", Box::new(move || {
                    let r = mm.s().running.clone();
                    let _ = mm.apps.open(&id2, &r);
                }));
                let id2 = id.clone();
                add("new_window", Box::new(move || {
                    let _ = mm_launch(&id2);
                }));
                let (mm, r2) = (m.clone(), running.clone());
                add("quit", Box::new(move || {
                    for a in &r2 {
                        mm.apps.quit(a);
                    }
                }));
                menu.append_section(None, &last);
            }
            Slot::AddApp => {
                menu.append(Some("Add Apps\u{2026}"), Some("bar.add"));
                let mm = m.clone();
                add("add", Box::new(move || actions::add_apps(&mm)));
            }
            Slot::Folder(f) => {
                menu.append(Some("Open"), Some("bar.open"));
                add("open", Box::new(move || folders::open(f)));
                if f == dock::DockFolder::Trash && self.trash_full.get() {
                    menu.append(Some("Empty Trash\u{2026}"), Some("bar.empty"));
                    add("empty", Box::new(|| empty_trash()));
                }
            }
            Slot::Divider | Slot::Separator => {}
        }
        let settings = gio::Menu::new();
        settings.append(Some("Kairos Bar Settings\u{2026}"), Some("bar.settings"));
        let mm = m.clone();
        add("settings", Box::new(move || {
            mm.set(change::NAV, |s| s.page = crate::model::Page::Settings);
            show_main_window(&mm);
        }));
        menu.append_section(None, &settings);
        self.area.insert_action_group("bar", Some(&group));
        let pop = gtk::PopoverMenu::from_model(Some(&menu));
        pop.set_parent(&self.area);
        pop.set_has_arrow(false);
        pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        pop.set_position(match self.frame.borrow().edge {
            DockEdge::Bottom => gtk::PositionType::Top,
            DockEdge::Left => gtk::PositionType::Right,
            DockEdge::Right => gtk::PositionType::Left,
        });
        self.menu_open.set(true);
        let weak = Rc::downgrade(self);
        pop.connect_closed(move |p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
            if let Some(me) = weak.upgrade() {
                me.menu_open.set(false);
                me.schedule(0);
            }
        });
        pop.popup();
        self.schedule(0);
    }
}

fn show_main_window(model: &Model) {
    let show = model.hooks.borrow().show_main_window.clone();
    if let Some(f) = show {
        f();
    }
}

fn mm_launch(id: &str) -> Result<(), String> {
    let info = Apps::info(id).ok_or("not found")?;
    let ctx = gdk::Display::default().map(|d| d.app_launch_context());
    info.launch(&[], ctx.as_ref()).map_err(|e| e.to_string())
}

fn empty_trash() {
    super::widgets::ask(None, "Empty the Trash?", "Everything in the Trash is deleted for good.", &["Empty Trash", "Cancel"], 1, |r| {
        if r != Some(0) {
            return;
        }
        let trash = gio::File::for_uri("trash:///");
        if let Ok(children) = trash.enumerate_children("standard::name", gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS, gio::Cancellable::NONE) {
            while let Ok(Some(info)) = children.next_file(gio::Cancellable::NONE) {
                let _ = trash.child(info.name()).delete(gio::Cancellable::NONE);
            }
        }
    });
}

fn slot_name(model: &Model, data: &AppData, slot: &Slot) -> Option<String> {
    Some(match slot {
        Slot::Kairos => "Kairos".into(),
        Slot::Group(id) => data.group(Some(*id))?.name.clone(),
        Slot::NewGroup => "New Group".into(),
        Slot::App(id) | Slot::Running(id) => model.apps.name(id),
        Slot::AddApp => "Add Apps".into(),
        Slot::Folder(f) => f.name().into(),
        Slot::Divider => "Drag to resize".into(),
        Slot::Separator => return None,
    })
}

fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    use std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

fn plus(cr: &gtk::cairo::Context, cx: f64, cy: f64, len: f64, (r, g, b, a): (f64, f64, f64, f64)) {
    cr.set_source_rgba(r, g, b, a);
    cr.set_line_width((len * 0.14).max(1.5));
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    cr.move_to(cx - len / 2.0, cy);
    cr.line_to(cx + len / 2.0, cy);
    cr.move_to(cx, cy - len / 2.0);
    cr.line_to(cx, cy + len / 2.0);
    let _ = cr.stroke();
}

fn paint_pixbuf(cr: &gtk::cairo::Context, pb: &gtk::gdk_pixbuf::Pixbuf, x: f64, y: f64, size: f64, alpha: f64) {
    let _ = cr.save();
    let sx = size / pb.width().max(1) as f64;
    cr.translate(x, y);
    cr.scale(sx, sx);
    cr.set_source_pixbuf(pb, 0.0, 0.0);
    cr.source().set_filter(gtk::cairo::Filter::Good);
    let _ = cr.paint_with_alpha(alpha);
    let _ = cr.restore();
}

/// A symbolic icon in one colour: the icon's own pixels used as a mask.
fn symbol(cr: &gtk::cairo::Context, name: &str, cx: f64, cy: f64, size: f64, (r, g, b): (f64, f64, f64)) {
    let px = size.round().max(8.0) as i32;
    let Some(pb) = icons::named(name, px) else { return };
    let Ok(surface) = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, pb.width(), pb.height()) else { return };
    {
        let Ok(c) = gtk::cairo::Context::new(&surface) else { return };
        c.set_source_pixbuf(&pb, 0.0, 0.0);
        let _ = c.paint();
    }
    let _ = cr.save();
    let s = size / pb.width().max(1) as f64;
    cr.translate(cx - size / 2.0, cy - size / 2.0);
    cr.scale(s, s);
    cr.set_source_rgb(r, g, b);
    let _ = cr.mask_surface(&surface, 0.0, 0.0);
    let _ = cr.restore();
}

