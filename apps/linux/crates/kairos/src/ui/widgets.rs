//! Pieces the pages share: cards, warning banners, task rows, list editors, the app picker, and
//! small dialogs.

use crate::model::{change, Model, Page};
use crate::platform::apps::Apps;
use crate::ui::icons;
use gtk::prelude::*;
use gtk::{gio, glib};
use kairos_core::duetext;
use kairos_core::*;
use std::cell::RefCell;
use std::rc::Rc;

pub const CSS: &str = r#"
.kairos-card { background: alpha(@theme_fg_color, 0.06); border-radius: 14px; padding: 16px; }
.kairos-stat { background: alpha(@theme_fg_color, 0.06); border-radius: 12px; padding: 14px; }
.kairos-warning { background: alpha(orange, 0.14); border: 1px solid alpha(orange, 0.35); border-radius: 10px; padding: 12px; }
.kairos-warning-text { color: #c86a00; }
.kairos-title { font-size: 20px; font-weight: 600; }
.kairos-big { font-size: 44px; font-weight: 600; font-feature-settings: "tnum"; }
.kairos-clock { font-size: 30px; font-weight: 500; font-feature-settings: "tnum"; }
.kairos-stat-value { font-size: 24px; font-weight: 600; font-feature-settings: "tnum"; }
.kairos-caption { font-size: 12px; opacity: 0.65; }
.kairos-heading { font-weight: 700; }
.kairos-dim { opacity: 0.6; }
.kairos-done { opacity: 0.55; text-decoration: line-through; }
.kairos-overdue { color: #d6455b; }
.kairos-pill { background: alpha(@theme_selected_bg_color, 0.22); border-radius: 999px; padding: 2px 8px; font-size: 12px; font-weight: 600; }
.kairos-tag { background: alpha(@theme_fg_color, 0.1); border-radius: 999px; padding: 2px 8px; font-size: 12px; }
.kairos-field { background: alpha(@theme_fg_color, 0.06); border-radius: 10px; padding: 4px 8px; }
.kairos-step { background: alpha(@theme_selected_bg_color, 0.25); border-radius: 999px; min-width: 26px; min-height: 26px; font-weight: 700; }
.kairos-swatch { border-radius: 999px; min-width: 22px; min-height: 22px; padding: 0; }
.kairos-swatch.chosen { box-shadow: 0 0 0 2px @theme_fg_color; }
.kairos-icon-choice.chosen { background: alpha(@theme_selected_bg_color, 0.3); }
window.kairos-transparent, window.kairos-transparent > * { background: none; background-color: transparent; box-shadow: none; }
.kairos-quickadd { background: alpha(@theme_bg_color, 0.97); border-radius: 16px; padding: 16px; border: 1px solid alpha(@theme_fg_color, 0.12); }
.kairos-quickadd entry { font-size: 20px; background: none; border: none; box-shadow: none; }
.kairos-overlay { background: rgba(0, 0, 0, 0.55); }
.kairos-overlay-card { background: @theme_bg_color; border-radius: 20px; padding: 28px; }
.kairos-overlay-icon { font-size: 34px; }
.kairos-lock { color: #e0703a; }
"#;

pub fn label(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l
}

pub fn styled(text: &str, class: &str) -> gtk::Label {
    let l = label(text);
    for c in class.split_whitespace() {
        l.add_css_class(c);
    }
    l
}

pub fn heading(text: &str) -> gtk::Label {
    styled(text, "kairos-heading")
}

pub fn caption(text: &str) -> gtk::Label {
    styled(text, "kairos-caption")
}

pub fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

pub fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

pub fn spacer() -> gtk::Box {
    let b = hbox(0);
    b.set_hexpand(true);
    b
}

pub fn button(text: &str, f: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.connect_clicked(move |_| f());
    b
}

pub fn icon_button(icon: &str, tip: &str, f: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.add_css_class("flat");
    b.set_tooltip_text(Some(tip));
    b.update_property(&[gtk::accessible::Property::Label(tip)]);
    b.connect_clicked(move |_| f());
    b
}

pub fn suggested(b: gtk::Button) -> gtk::Button {
    b.add_css_class("suggested-action");
    b
}

pub fn card(child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = vbox(8);
    b.add_css_class("kairos-card");
    b.append(child);
    b
}

/// Something that is not working the way the owner probably thinks it is.
pub fn warning(text: &str, action: Option<gtk::Widget>) -> gtk::Box {
    let b = hbox(10);
    b.add_css_class("kairos-warning");
    let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
    icon.add_css_class("kairos-warning-text");
    b.append(&icon);
    let l = label(text);
    l.set_hexpand(true);
    b.append(&l);
    if let Some(a) = action {
        a.set_valign(gtk::Align::Center);
        b.append(&a);
    }
    b
}

pub fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let s = gtk::ScrolledWindow::new();
    s.set_hscrollbar_policy(gtk::PolicyType::Never);
    s.set_vexpand(true);
    s.set_child(Some(child));
    s
}

pub fn page_box() -> gtk::Box {
    let b = vbox(18);
    b.set_margin_top(20);
    b.set_margin_bottom(20);
    b.set_margin_start(20);
    b.set_margin_end(20);
    b
}

pub fn dropdown(options: &[String], selected: usize, f: impl Fn(usize) + 'static) -> gtk::DropDown {
    let strs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
    let d = gtk::DropDown::from_strings(&strs);
    d.set_selected(selected.min(options.len().saturating_sub(1)) as u32);
    d.connect_selected_notify(move |d| f(d.selected() as usize));
    d
}

pub fn switch_row(title: &str, note: Option<&str>, on: bool, sensitive: bool, f: impl Fn(bool) + 'static) -> gtk::Box {
    let row = hbox(12);
    let text = vbox(2);
    text.set_hexpand(true);
    text.append(&label(title));
    if let Some(n) = note {
        text.append(&caption(n));
    }
    row.append(&text);
    let sw = gtk::Switch::new();
    sw.set_active(on);
    sw.set_valign(gtk::Align::Center);
    sw.set_sensitive(sensitive);
    sw.update_property(&[gtk::accessible::Property::Label(title)]);
    sw.connect_active_notify(move |s| f(s.is_active()));
    row.append(&sw);
    row
}

pub fn labeled(title: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = hbox(12);
    let l = label(title);
    l.set_hexpand(true);
    row.append(&l);
    widget.as_ref().set_valign(gtk::Align::Center);
    row.append(widget);
    row
}

pub fn section(title: &str) -> (gtk::Box, gtk::Box) {
    let outer = vbox(8);
    outer.append(&styled(title, "kairos-heading"));
    let inner = vbox(10);
    inner.add_css_class("kairos-card");
    outer.append(&inner);
    (outer, inner)
}

pub fn spin(min: f64, max: f64, step: f64, value: f64, f: impl Fn(f64) + 'static) -> gtk::SpinButton {
    let s = gtk::SpinButton::with_range(min, max, step);
    s.set_value(value);
    s.connect_value_changed(move |s| f(s.value()));
    s
}

/// A text field that commits on Enter or when it loses focus, not on every key.
pub fn entry(text: &str, placeholder: &str, commit: impl Fn(String) + 'static) -> gtk::Entry {
    let e = gtk::Entry::new();
    e.set_text(text);
    e.set_placeholder_text(Some(placeholder));
    let commit = Rc::new(commit);
    let c = commit.clone();
    e.connect_activate(move |e| c(e.text().to_string()));
    let focus = gtk::EventControllerFocus::new();
    let weak = e.downgrade();
    let original = RefCell::new(text.to_string());
    focus.connect_leave(move |_| {
        if let Some(e) = weak.upgrade() {
            let t = e.text().to_string();
            if *original.borrow() != t {
                *original.borrow_mut() = t.clone();
                commit(t);
            }
        }
    });
    e.add_controller(focus);
    e
}

// MARK: Tasks

pub fn task_row(model: &Rc<Model>, task: &TaskItem, show_group: bool) -> gtk::Box {
    let s = model.s();
    let row = hbox(10);
    row.set_margin_top(3);
    row.set_margin_bottom(3);
    let done = task.is_done();
    let check = gtk::CheckButton::new();
    check.set_active(done);
    check.update_property(&[gtk::accessible::Property::Label(if done { "Mark not done" } else { "Mark done" })]);
    {
        let (m, id) = (model.clone(), task.id);
        check.connect_toggled(move |c| {
            let (m, active) = (m.clone(), c.is_active());
            glib::idle_add_local_once(move || m.set_done(id, active));
        });
    }
    row.append(&check);
    let text = vbox(2);
    text.set_hexpand(true);
    let title = label(&task.title);
    if done {
        title.add_css_class("kairos-done");
    }
    text.append(&title);
    let meta = hbox(10);
    meta.add_css_class("kairos-caption");
    if show_group {
        if let Some(g) = s.data.group(task.group_id) {
            let b = hbox(4);
            b.append(&gtk::Image::from_icon_name(icons::group_icon(&g.icon)));
            b.append(&gtk::Label::new(Some(&g.name)));
            meta.append(&b);
        }
    }
    if let Some(due) = task.due {
        let l = gtk::Label::new(Some(&format!("\u{1F4C5} {}", duetext::short_local(due, s.now))));
        l.set_tooltip_text(Some(&time::local(due).format("%A %-d %B %Y, %H:%M").to_string()));
        if due < time::now() && !done {
            l.add_css_class("kairos-overdue");
        }
        meta.append(&l);
    }
    if let Some(e) = task.estimate_min {
        meta.append(&gtk::Label::new(Some(&format!("\u{23F1} {e} min"))));
    }
    if task.priority > 0 {
        meta.append(&gtk::Label::new(Some(&format!("\u{2691} {}", ["", "Low", "Medium", "High"][task.priority.clamp(0, 3) as usize]))));
    }
    let subs = s.subtasks(task.id);
    if !subs.is_empty() {
        meta.append(&gtk::Label::new(Some(&format!("\u{2630} {}/{}", subs.iter().filter(|t| t.is_done()).count(), subs.len()))));
    }
    if meta.first_child().is_some() {
        text.append(&meta);
    }
    row.append(&text);
    if s.data.focus.current_task_id == Some(task.id) {
        let pill = styled("Current", "kairos-pill");
        pill.set_valign(gtk::Align::Center);
        row.append(&pill);
    } else if !done {
        let idle = s.tempo.phase() == kairos_core::tempo::TempoPhase::Idle;
        let (m, t) = (model.clone(), task.clone());
        let b = button(if idle { "Start" } else { "Work on this" }, move || {
            let (m, t) = (m.clone(), t.clone());
            glib::idle_add_local_once(move || m.start_task(&t));
        });
        b.set_tooltip_text(Some(if idle { "Make this the current task and start a focus session" } else { "Make this the current task" }));
        b.set_valign(gtk::Align::Center);
        row.append(&b);
    }
    // Right-click to delete.
    let menu = gio::Menu::new();
    menu.append(Some("Delete"), Some("task.delete"));
    let pop = gtk::PopoverMenu::from_model(Some(&menu));
    pop.set_parent(&row);
    pop.set_has_arrow(false);
    let group = gio::SimpleActionGroup::new();
    let delete = gio::SimpleAction::new("delete", None);
    {
        let (m, id) = (model.clone(), task.id);
        delete.connect_activate(move |_, _| {
            let m = m.clone();
            glib::idle_add_local_once(move || m.delete_task(id));
        });
    }
    group.add_action(&delete);
    row.insert_action_group("task", Some(&group));
    let click = gtk::GestureClick::new();
    click.set_button(3);
    click.connect_pressed(move |_, _, x, y| {
        pop.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        pop.popup();
    });
    row.add_controller(click);
    row
}

pub fn quick_add_field(model: &Rc<Model>) -> gtk::Box {
    let b = hbox(8);
    b.add_css_class("kairos-field");
    b.append(&gtk::Image::from_icon_name("list-add-symbolic"));
    let e = gtk::Entry::new();
    e.set_hexpand(true);
    e.set_has_frame(false);
    e.set_placeholder_text(Some("Add a task, like \u{201C}Report tomorrow 3pm\u{201D}"));
    e.set_tooltip_text(Some("Type a title, then any of: a day and time (tomorrow 3pm), @Group, ~45m for an estimate, !high for priority."));
    let m = model.clone();
    e.connect_activate(move |e| {
        let text = e.text().to_string();
        if m.add_task(&text).is_some() {
            e.set_text("");
        }
    });
    b.append(&e);
    b
}

// MARK: List editors

/// A list of strings with add and remove; `can_add`/`can_remove` follow the strict rule lock.
pub fn string_list(
    title: &str,
    placeholder: &str,
    items: &[String],
    normalize: fn(&str) -> String,
    can_add: bool,
    can_remove: bool,
    set: impl Fn(Vec<String>) + 'static,
) -> gtk::Box {
    let set = Rc::new(set);
    let b = vbox(6);
    b.append(&heading(title));
    for item in items {
        let row = hbox(8);
        let l = label(item);
        l.set_selectable(true);
        l.set_hexpand(true);
        row.append(&l);
        if can_remove {
            let (set, items, item) = (set.clone(), items.to_vec(), item.clone());
            row.append(&icon_button("list-remove-symbolic", &format!("Remove {item}"), move || {
                set(items.iter().filter(|i| **i != item).cloned().collect());
            }));
        }
        b.append(&row);
    }
    let add_row = hbox(8);
    let e = gtk::Entry::new();
    e.set_placeholder_text(Some(placeholder));
    e.set_hexpand(true);
    let add_btn = gtk::Button::with_label("Add");
    add_btn.set_sensitive(false);
    {
        let a = add_btn.clone();
        e.connect_changed(move |e| a.set_sensitive(!normalize(&e.text()).is_empty()));
    }
    let add = {
        let (set, items, e) = (set.clone(), items.to_vec(), e.clone());
        Rc::new(move || {
            let v = normalize(&e.text());
            if v.is_empty() {
                return;
            }
            let mut next = items.clone();
            if !next.contains(&v) {
                next.push(v);
            }
            e.set_text("");
            set(next);
        })
    };
    {
        let add = add.clone();
        e.connect_activate(move |_| add());
    }
    add_btn.connect_clicked(move |_| add());
    add_row.append(&e);
    add_row.append(&add_btn);
    add_row.set_sensitive(can_add);
    b.append(&add_row);
    b
}

pub fn trim(s: &str) -> String {
    s.trim().to_string()
}

pub fn normalize_site(s: &str) -> String {
    kairos_core::rules::normalize_site(s)
}

/// A list of apps with an "Add apps…" picker, optional up and down buttons, and removal.
pub fn app_list(model: &Rc<Model>, title: &str, empty: &str, ids: &[String], reorderable: bool, can_remove: bool, set: impl Fn(Vec<String>) + 'static) -> gtk::Box {
    let set = Rc::new(set);
    let b = vbox(6);
    let head = hbox(8);
    let h = heading(title);
    h.set_hexpand(true);
    head.append(&h);
    {
        let (set, ids, m) = (set.clone(), ids.to_vec(), model.clone());
        head.append(&button("Add apps\u{2026}", move || {
            let set = set.clone();
            app_picker(&m, ids.clone(), move |next| set(next));
        }));
    }
    b.append(&head);
    if ids.is_empty() {
        b.append(&caption(empty));
    }
    for (i, id) in ids.iter().enumerate() {
        let row = hbox(8);
        let img = match Apps::icon(id) {
            Some(icon) => gtk::Image::from_gicon(&icon),
            None => gtk::Image::from_icon_name("application-x-executable"),
        };
        img.set_pixel_size(22);
        row.append(&img);
        let name = model.apps.name(id);
        let l = label(&name);
        l.set_hexpand(true);
        row.append(&l);
        if reorderable {
            let (s1, ids1, id1) = (set.clone(), ids.to_vec(), id.clone());
            let up = icon_button("go-up-symbolic", &format!("Move {name} up"), move || s1(dock::moving_by(&ids1, &id1, -1)));
            up.set_sensitive(i > 0);
            row.append(&up);
            let (s2, ids2, id2) = (set.clone(), ids.to_vec(), id.clone());
            let down = icon_button("go-down-symbolic", &format!("Move {name} down"), move || s2(dock::moving_by(&ids2, &id2, 1)));
            down.set_sensitive(i + 1 < ids.len());
            row.append(&down);
        }
        if can_remove {
            let (s3, ids3, id3) = (set.clone(), ids.to_vec(), id.clone());
            row.append(&icon_button("list-remove-symbolic", &format!("Remove {name}"), move || s3(ids3.iter().filter(|x| **x != id3).cloned().collect())));
        }
        b.append(&row);
    }
    b
}

/// Ticks apps on or off from every app the menu shows. Return ticks the first match.
pub fn app_picker(model: &Rc<Model>, selection: Vec<String>, done: impl Fn(Vec<String>) + 'static) {
    let win = gtk::Window::builder().title("Choose apps").default_width(420).default_height(520).modal(true).build();
    if let Some(parent) = main_window(model) {
        win.set_transient_for(Some(&parent));
    }
    let chosen = Rc::new(RefCell::new(selection));
    let done = Rc::new(done);
    let root = vbox(0);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search apps"));
    search.set_margin_top(12);
    search.set_margin_start(12);
    search.set_margin_end(12);
    search.set_margin_bottom(8);
    root.append(&search);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    let apps = Apps::installed();
    let mut rows: Vec<(gtk::ListBoxRow, gtk::CheckButton, String)> = vec![];
    for app in &apps {
        let row = gtk::ListBoxRow::new();
        let b = hbox(10);
        b.set_margin_top(4);
        b.set_margin_bottom(4);
        b.set_margin_start(10);
        let check = gtk::CheckButton::new();
        check.set_active(chosen.borrow().contains(&app.id));
        check.update_property(&[gtk::accessible::Property::Label(&app.name)]);
        {
            let (chosen, id, done) = (chosen.clone(), app.id.clone(), done.clone());
            check.connect_toggled(move |c| {
                {
                    let mut list = chosen.borrow_mut();
                    if c.is_active() {
                        if !list.contains(&id) {
                            list.push(id.clone());
                        }
                    } else {
                        list.retain(|x| *x != id);
                    }
                }
                done(chosen.borrow().clone());
            });
        }
        b.append(&check);
        let img = match &app.icon {
            Some(i) => gtk::Image::from_gicon(i),
            None => gtk::Image::from_icon_name("application-x-executable"),
        };
        img.set_pixel_size(26);
        b.append(&img);
        b.append(&gtk::Label::new(Some(&app.name)));
        row.set_child(Some(&b));
        list.append(&row);
        rows.push((row, check, app.name.to_lowercase()));
    }
    let rows = Rc::new(rows);
    {
        let rows = rows.clone();
        search.connect_search_changed(move |s| {
            let q = s.text().to_lowercase();
            for (row, _, name) in rows.iter() {
                row.set_visible(q.is_empty() || name.contains(&q));
            }
        });
    }
    {
        let rows = rows.clone();
        search.connect_activate(move |_| {
            if let Some((_, check, _)) = rows.iter().find(|(r, _, _)| r.is_visible()) {
                check.set_active(!check.is_active());
            }
        });
    }
    root.append(&scrolled(&list));
    let foot = hbox(8);
    foot.set_margin_top(12);
    foot.set_margin_bottom(12);
    foot.set_margin_end(12);
    foot.append(&spacer());
    let w = win.clone();
    let done_btn = suggested(button("Done", move || w.close()));
    foot.append(&done_btn);
    root.append(&foot);
    win.set_child(Some(&root));
    win.present();
}

// MARK: Dialogs

pub fn main_window(model: &Rc<Model>) -> Option<gtk::Window> {
    let _ = model;
    gtk::gio::Application::default()
        .and_then(|a| a.downcast::<gtk::Application>().ok())
        .and_then(|a| a.windows().into_iter().find(|w| w.widget_name() == "kairos-main"))
}

/// Asks a question with buttons; `then` gets the index of the button pressed (None for cancel).
pub fn ask(parent: Option<&gtk::Window>, title: &str, detail: &str, buttons: &[&str], cancel: usize, then: impl Fn(Option<usize>) + 'static) {
    let d = gtk::AlertDialog::builder().message(title).detail(detail).buttons(buttons.to_vec()).cancel_button(cancel as i32).default_button(0).modal(true).build();
    d.choose(parent, gio::Cancellable::NONE, move |r| then(r.ok().map(|i| i as usize).filter(|i| *i != cancel)));
}

pub fn alert(parent: Option<&gtk::Window>, title: &str, detail: &str) {
    let d = gtk::AlertDialog::builder().message(title).detail(detail).modal(true).build();
    d.show(parent);
}

/// Asks for a line of text, such as a group name.
pub fn ask_text(parent: Option<&gtk::Window>, title: &str, info: Option<&str>, value: &str, action: &str, then: impl Fn(String) + 'static) {
    let win = gtk::Window::builder().title(title).modal(true).resizable(false).default_width(340).build();
    if let Some(p) = parent {
        win.set_transient_for(Some(p));
    }
    let b = vbox(12);
    b.set_margin_top(18);
    b.set_margin_bottom(18);
    b.set_margin_start(18);
    b.set_margin_end(18);
    b.append(&styled(title, "kairos-heading"));
    if let Some(i) = info {
        b.append(&caption(i));
    }
    let e = gtk::Entry::new();
    e.set_text(value);
    e.set_placeholder_text(Some("Group name"));
    b.append(&e);
    let row = hbox(8);
    row.append(&spacer());
    let w = win.clone();
    row.append(&button("Cancel", move || w.close()));
    let then = Rc::new(then);
    let ok = {
        let (w, e, then) = (win.clone(), e.clone(), then.clone());
        Rc::new(move || {
            let t = e.text().trim().to_string();
            w.close();
            if !t.is_empty() {
                then(t);
            }
        })
    };
    let ok2 = ok.clone();
    row.append(&suggested(button(action, move || ok2())));
    e.connect_activate(move |_| ok());
    b.append(&row);
    win.set_child(Some(&b));
    let esc = gtk::EventControllerKey::new();
    let w = win.clone();
    esc.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            w.close();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    win.add_controller(esc);
    win.present();
    e.grab_focus();
}

/// Group management started from Kairos Bar or the tray.
pub mod actions {
    use super::*;

    pub fn new_group(model: &Rc<Model>) {
        let m = model.clone();
        ask_text(None, "New Group", Some("It starts empty. Add apps from Kairos Bar, or drag them onto it."), "", "Create", move |name| {
            let id = m.add_group(&name);
            m.switch_group(id);
        });
    }

    pub fn rename(model: &Rc<Model>, id: Id) {
        let Some(name) = model.s().data.group(Some(id)).map(|g| g.name.clone()) else { return };
        let m = model.clone();
        ask_text(None, &format!("Rename \u{201C}{name}\u{201D}"), None, &name, "Rename", move |n| m.rename_group(id, &n));
    }

    pub fn delete(model: &Rc<Model>, id: Id) {
        let Some(name) = model.s().data.group(Some(id)).map(|g| g.name.clone()) else { return };
        if model.s().data.groups.len() < 2 {
            return;
        }
        let m = model.clone();
        ask(
            main_window(model).as_ref(),
            &format!("Delete \u{201C}{name}\u{201D}?"),
            "Its apps and blocking rules are removed. Its tasks are kept, with no group.",
            &["Delete", "Cancel"],
            1,
            move |r| {
                if r == Some(0) {
                    m.delete_group(id);
                }
            },
        );
    }

    pub fn add_apps(model: &Rc<Model>) {
        let current = model.s().active_group().map(|g| g.dock_apps.clone()).unwrap_or_default();
        let m = model.clone();
        app_picker(model, current, move |next| {
            let active = m.s().data.focus.active_group_id;
            if let Some(id) = active {
                m.update_group(id, |g| g.dock_apps = next);
            }
        });
    }

    pub fn edit_groups(model: &Rc<Model>, id: Option<Id>) {
        let target = id.or(model.s().data.focus.active_group_id);
        model.set(change::NAV, |s| {
            s.selected_group_id = target;
            s.page = Page::Groups;
        });
        let show = model.hooks.borrow().show_main_window.clone();
        if let Some(f) = show {
            f();
        }
    }
}
