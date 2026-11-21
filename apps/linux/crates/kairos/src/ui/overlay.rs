//! The block overlay ("Slack is hidden") and the full-screen break reminder. One window, shown
//! while the model has a notice, above everything on every workspace.

use super::widgets::*;
use crate::model::{change, Model, NoticeKind, State};
use crate::platform::x11::X11;
use gtk::glib;
use gtk::prelude::*;
use kairos_core::tempo::clock;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

pub struct Overlay {
    model: Rc<Model>,
    x: Option<Arc<X11>>,
    window: RefCell<Option<gtk::Window>>,
    shown: Cell<Option<u64>>,
    clock: RefCell<Option<gtk::Label>>,
    app: gtk::Application,
}

impl Overlay {
    pub fn new(app: &gtk::Application, model: &Rc<Model>, x: Option<Arc<X11>>) -> Rc<Overlay> {
        let me = Rc::new(Overlay { model: model.clone(), x, window: RefCell::new(None), shown: Cell::new(None), clock: RefCell::new(None), app: app.clone() });
        let weak = Rc::downgrade(&me);
        model.subscribe(move |what| {
            let Some(me) = weak.upgrade() else { return };
            if what & change::NOTICE != 0 {
                me.sync();
            }
            if what & change::TICK != 0 {
                if let Some(l) = me.clock.borrow().as_ref() {
                    l.set_text(&rest_text(&me.model.s()));
                }
            }
        });
        me
    }

    fn sync(self: &Rc<Self>) {
        let notice = self.model.s().notice.clone();
        match notice {
            None => {
                self.shown.set(None);
                self.clock.borrow_mut().take();
                if let Some(w) = self.window.borrow_mut().take() {
                    w.destroy();
                }
            }
            Some(n) if Some(n.id) != self.shown.get() => {
                self.shown.set(Some(n.id));
                if let Some(w) = self.window.borrow_mut().take() {
                    w.destroy();
                }
                self.clock.borrow_mut().take();
                let rest = n.kind == NoticeKind::Rest;
                let win = gtk::Window::builder().application(&self.app).decorated(false).title(if rest { "Break" } else { "Blocked" }).build();
                win.add_css_class("kairos-transparent");
                let backdrop = gtk::CenterBox::new();
                backdrop.add_css_class("kairos-overlay");
                backdrop.set_hexpand(true);
                backdrop.set_vexpand(true);
                let card = vbox(14);
                card.add_css_class("kairos-overlay-card");
                card.set_valign(gtk::Align::Center);
                card.set_halign(gtk::Align::Center);
                card.set_size_request(420, -1);
                let icon = gtk::Image::from_icon_name(if rest { "weather-clear-night-symbolic" } else { "action-unavailable-symbolic" });
                icon.set_pixel_size(42);
                card.append(&icon);
                let title = styled(&n.title, "kairos-title");
                title.set_xalign(0.5);
                card.append(&title);
                let detail = styled(&n.detail, "kairos-dim");
                detail.set_xalign(0.5);
                detail.set_justify(gtk::Justification::Center);
                card.append(&detail);
                let buttons = hbox(8);
                buttons.set_halign(gtk::Align::Center);
                if rest {
                    let c = styled(&rest_text(&self.model.s()), "kairos-big");
                    c.set_xalign(0.5);
                    card.append(&c);
                    *self.clock.borrow_mut() = Some(c);
                    let strict = self.model.s().strict_locked();
                    let skip = button("Skip break", {
                        let m = self.model.clone();
                        move || {
                            let m = m.clone();
                            glib::idle_add_local_once(move || m.dismiss_notice());
                        }
                    });
                    skip.set_sensitive(!strict);
                    buttons.append(&skip);
                    let hide = button("Hide", {
                        let m = self.model.clone();
                        move || m.hide_notice()
                    });
                    buttons.append(&hide);
                } else {
                    if self.model.s().notice_can_override() {
                        let minutes = self.model.s().data.settings.override_minutes;
                        let left = self.model.s().overrides_left();
                        let reason = gtk::Entry::new();
                        reason.set_placeholder_text(Some("Why do you need it? (optional)"));
                        card.append(&reason);
                        let error = styled("", "kairos-warning-text");
                        error.set_visible(false);
                        let allow = button(&format!("Allow {minutes} min ({left} left)"), {
                            let (m, reason, error) = (self.model.clone(), reason.clone(), error.clone());
                            move || {
                                if let Some(e) = m.override_notice(reason.text().trim()) {
                                    error.set_text(&e);
                                    error.set_visible(true);
                                }
                            }
                        });
                        {
                            let a = allow.clone();
                            reason.connect_activate(move |_| a.emit_clicked());
                        }
                        buttons.append(&allow);
                        card.append(&error);
                    }
                    let ok = suggested(button("OK", {
                        let m = self.model.clone();
                        move || {
                            let m = m.clone();
                            glib::idle_add_local_once(move || m.dismiss_notice());
                        }
                    }));
                    buttons.append(&ok);
                    win.set_default_widget(Some(&ok));
                }
                card.append(&buttons);
                backdrop.set_center_widget(Some(&card));
                win.set_child(Some(&backdrop));
                let esc = gtk::EventControllerKey::new();
                {
                    let m = self.model.clone();
                    esc.connect_key_pressed(move |_, key, _, _| {
                        if key == gtk::gdk::Key::Escape {
                            m.hide_notice();
                            return glib::Propagation::Stop;
                        }
                        glib::Propagation::Proceed
                    });
                }
                win.add_controller(esc);
                {
                    let x = self.x.clone();
                    win.connect_realize(move |w| {
                        let Some(x) = &x else { return };
                        if let Some(xid) = w.surface().and_then(|s| s.downcast::<gdk4_x11::X11Surface>().ok()).map(|s| s.xid() as u32) {
                            x.make_overlay(xid, rest);
                        }
                    });
                }
                win.fullscreen();
                win.present();
                *self.window.borrow_mut() = Some(win);
            }
            _ => {}
        }
    }
}

fn rest_text(s: &State) -> String {
    clock(s.remaining())
}
