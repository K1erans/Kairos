//! The main window: a sidebar of pages and the page itself. Pages are rebuilt from the model
//! when what they show changes, except while the owner is typing in one or dragging a slider,
//! so a block logged by the browser never eats a half-typed task. Timer readouts update every second in place.

use super::pages;
use super::widgets::*;
use crate::model::{change, Model, Page, State};
use crate::platform::system::{autostart, bridge_installer};
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub type Tickers = Vec<Box<dyn Fn(&State)>>;

pub struct MainWindow {
    pub window: gtk::ApplicationWindow,
    model: Rc<Model>,
    content: gtk::Box,
    sidebar: gtk::ListBox,
    tickers: RefCell<Tickers>,
    shown: Cell<Option<Page>>,
    dirty: Cell<bool>,
    /// What the shown page read from outside Kairos, to tell on activation whether it is stale.
    outside: RefCell<Option<Outside>>,
    onboarding: RefCell<Option<gtk::Window>>,
}

/// Things the pages show that can change outside Kairos: start at login and the browsers the
/// bridge is installed for.
#[derive(PartialEq)]
struct Outside {
    open_at_login: bool,
    browsers: Vec<String>,
}

impl Outside {
    fn read() -> Self {
        Outside { open_at_login: autostart::enabled(), browsers: bridge_installer::installed_browsers() }
    }
}

impl MainWindow {
    pub fn new(app: &gtk::Application, model: &Rc<Model>) -> Rc<MainWindow> {
        let window = gtk::ApplicationWindow::builder().application(app).title("Kairos").default_width(1000).default_height(660).build();
        window.set_widget_name("kairos-main");
        window.set_size_request(860, 560);
        window.set_hide_on_close(true);
        let root = gtk::Paned::new(gtk::Orientation::Horizontal);
        let sidebar = gtk::ListBox::new();
        sidebar.add_css_class("navigation-sidebar");
        sidebar.set_size_request(180, -1);
        for p in Page::ALL {
            let row = gtk::ListBoxRow::new();
            let b = hbox(10);
            b.set_margin_top(8);
            b.set_margin_bottom(8);
            b.set_margin_start(10);
            b.append(&gtk::Image::from_icon_name(p.icon()));
            b.append(&gtk::Label::new(Some(p.title())));
            row.set_child(Some(&b));
            sidebar.append(&row);
        }
        root.set_start_child(Some(&sidebar));
        root.set_shrink_start_child(false);
        root.set_resize_start_child(false);
        let content = vbox(0);
        content.set_hexpand(true);
        content.set_vexpand(true);
        root.set_end_child(Some(&content));
        window.set_child(Some(&root));
        let me = Rc::new(MainWindow {
            window,
            model: model.clone(),
            content,
            sidebar,
            tickers: RefCell::new(vec![]),
            shown: Cell::new(None),
            dirty: Cell::new(true),
            outside: RefCell::new(None),
            onboarding: RefCell::new(None),
        });
        {
            let m = model.clone();
            me.sidebar.connect_row_selected(move |_, row| {
                let Some(row) = row else { return };
                let page = Page::ALL[row.index().max(0) as usize];
                if m.s().page != page {
                    m.set(change::NAV, |s| s.page = page);
                }
            });
        }
        {
            let weak = Rc::downgrade(&me);
            model.subscribe(move |what| {
                if let Some(me) = weak.upgrade() {
                    me.changed(what);
                }
            });
        }
        {
            // Finishing an edit lets a deferred rebuild happen.
            let weak = Rc::downgrade(&me);
            me.window.connect_focus_widget_notify(move |_| {
                if let Some(me) = weak.upgrade() {
                    if me.dirty.get() {
                        let me2 = me.clone();
                        glib::idle_add_local_once(move || me2.rebuild());
                    }
                }
            });
        }
        {
            let weak = Rc::downgrade(&me);
            me.window.connect_is_active_notify(move |w| {
                if let Some(me) = weak.upgrade() {
                    if w.is_active() {
                        // Things that change outside Kairos: start at login, browsers. Rebuild only
                        // when one did: a rebuild replaces every widget, so rebuilding on every
                        // activation closed a drop-down's list as the click that opened it
                        // activated the window.
                        if me.outside.borrow().as_ref() != Some(&Outside::read()) {
                            me.dirty.set(true);
                            me.rebuild();
                        }
                        me.model.schedule_reminders();
                    }
                }
            });
        }
        me
    }

    pub fn present(self: &Rc<Self>) {
        self.rebuild();
        self.window.present();
        self.check_onboarding();
    }

    fn changed(self: &Rc<Self>, what: u32) {
        if what & change::TICK != 0 && self.window.is_visible() {
            let s = self.model.s();
            for t in self.tickers.borrow().iter() {
                t(&s);
            }
        }
        let relevant = what & !change::TICK;
        if relevant == 0 {
            return;
        }
        // Today and Settings show bridge clients and keys; every page shows data.
        self.dirty.set(true);
        if self.window.is_visible() {
            self.rebuild();
        }
        if what & change::DATA != 0 {
            self.check_onboarding();
        }
    }

    /// A text field or slider on the page has focus. A slider takes focus when pressed and
    /// changes the data on every step of a drag, so rebuilding under it would end the drag.
    fn typing(&self) -> bool {
        let Some(focus) = gtk::prelude::GtkWindowExt::focus(&self.window) else { return false };
        let editable = focus.is::<gtk::Text>()
            || focus.is::<gtk::TextView>()
            || focus.is::<gtk::Entry>()
            || focus.is::<gtk::SpinButton>()
            || focus.is::<gtk::Range>();
        editable && focus.is_ancestor(&self.content)
    }

    pub fn rebuild(self: &Rc<Self>) {
        if !self.dirty.get() && self.shown.get() == Some(self.model.s().page) {
            return;
        }
        let page = self.model.s().page;
        if self.typing() && self.shown.get() == Some(page) {
            return; // rebuilt when the field loses focus
        }
        self.dirty.set(false);
        *self.outside.borrow_mut() = Some(Outside::read());
        let idx = Page::ALL.iter().position(|p| *p == page).unwrap_or(0) as i32;
        if self.sidebar.selected_row().map(|r| r.index()) != Some(idx) {
            if let Some(row) = self.sidebar.row_at_index(idx) {
                self.sidebar.select_row(Some(&row));
            }
        }
        // Keep the scroll position across a rebuild of the same page.
        let scroll = if self.shown.get() == Some(page) { first_scroll(&self.content).map(|s| s.vadjustment().value()) } else { None };
        while let Some(c) = self.content.first_child() {
            self.content.remove(&c);
        }
        let mut tickers: Tickers = vec![];
        let widget = pages::build(&self.model, page, &mut tickers);
        self.content.append(&widget);
        *self.tickers.borrow_mut() = tickers;
        self.shown.set(Some(page));
        self.window.set_title(Some(&format!("Kairos \u{2013} {}", page.title())));
        if let Some(v) = scroll {
            let content = self.content.clone();
            glib::idle_add_local_once(move || {
                if let Some(s) = first_scroll(&content) {
                    s.vadjustment().set_value(v);
                }
            });
        }
    }

    fn check_onboarding(self: &Rc<Self>) {
        let needed = !self.model.s().data.onboarded && self.window.is_visible();
        let open = self.onboarding.borrow().is_some();
        if needed && !open {
            let w = pages::onboarding(&self.model, &self.window);
            let weak = Rc::downgrade(self);
            w.connect_close_request(move |_| {
                if let Some(me) = weak.upgrade() {
                    me.onboarding.borrow_mut().take();
                    if !me.model.s().data.onboarded {
                        me.model.edit(|d| d.onboarded = true);
                    }
                }
                glib::Propagation::Proceed
            });
            w.present();
            *self.onboarding.borrow_mut() = Some(w);
        } else if !needed && open {
            // Release the borrow before closing: close() runs the close handler, which borrows it too.
            let w = self.onboarding.borrow_mut().take();
            if let Some(w) = w {
                w.close();
            }
        }
    }
}

fn first_scroll(w: &gtk::Box) -> Option<gtk::ScrolledWindow> {
    fn find(w: &gtk::Widget) -> Option<gtk::ScrolledWindow> {
        if let Ok(s) = w.clone().downcast::<gtk::ScrolledWindow>() {
            return Some(s);
        }
        let mut c = w.first_child();
        while let Some(child) = c {
            if let Some(s) = find(&child) {
                return Some(s);
            }
            c = child.next_sibling();
        }
        None
    }
    find(w.upcast_ref())
}
