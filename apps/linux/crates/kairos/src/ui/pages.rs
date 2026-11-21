//! The pages of the main window: Today, Tasks, Groups, Rules, Stats and Settings, and the
//! first-launch welcome.

use super::icons;
use super::widgets::*;
use super::window::Tickers;
use crate::model::{change, Model, Page, SITE_BLOCKING_OFF_TEXT};
use crate::platform::system::{self, autostart, bridge_installer};
use chrono::{Datelike, Duration as ChronoDuration, Local, TimeZone};
use gtk::prelude::*;
use gtk::{gio, glib};
use kairos_core::hotkeys::{self, HotkeyPlan};
use kairos_core::setup::{SetupItem, SetupStep};
use kairos_core::tempo::{clock, TempoPhase};
use kairos_core::*;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

pub fn build(model: &Rc<Model>, page: Page, tickers: &mut Tickers) -> gtk::Widget {
    match page {
        Page::Today => today(model, tickers),
        Page::Tasks => tasks(model),
        Page::Groups => groups(model),
        Page::Rules => rules_page(model),
        Page::Stats => stats(model),
        Page::Settings => settings(model),
    }
}

fn later(model: &Rc<Model>, f: impl FnOnce(&Rc<Model>) + 'static) -> impl Fn() + 'static {
    let m = model.clone();
    let f = RefCell::new(Some(f));
    move || {
        let m = m.clone();
        if let Some(f) = f.borrow_mut().take() {
            glib::idle_add_local_once(move || f(&m));
        }
    }
}

/// Runs `f` after the click that asked for it, since it rebuilds the page under the button.
fn act(model: &Rc<Model>, f: impl Fn(&Rc<Model>) + 'static) -> impl Fn() + 'static {
    let m = model.clone();
    let f = Rc::new(f);
    move || {
        let (m, f) = (m.clone(), f.clone());
        glib::idle_add_local_once(move || f(&m));
    }
}

/// The group switcher: one toggle per group, joined.
fn group_picker(model: &Rc<Model>, selected: Option<Id>, on: impl Fn(Id) + 'static) -> gtk::Box {
    let b = hbox(0);
    b.add_css_class("linked");
    let on = Rc::new(on);
    for g in model.s().data.groups.iter() {
        let t = gtk::ToggleButton::new();
        let inner = hbox(6);
        inner.append(&gtk::Image::from_icon_name(icons::group_icon(&g.icon)));
        inner.append(&gtk::Label::new(Some(&g.name)));
        t.set_child(Some(&inner));
        t.set_active(Some(g.id) == selected);
        let (on, id) = (on.clone(), g.id);
        t.connect_clicked(move |t| {
            if t.is_active() {
                let on = on.clone();
                glib::idle_add_local_once(move || on(id));
            } else {
                t.set_active(true);
            }
        });
        b.append(&t);
    }
    b
}

/// The one Mode control. While a timer runs the timer owns the mode, so it is locked.
fn mode_picker(model: &Rc<Model>) -> gtk::Box {
    let b = hbox(0);
    b.add_css_class("linked");
    let (mode, idle) = {
        let s = model.s();
        (s.mode(), s.tempo.phase() == TempoPhase::Idle)
    };
    for m in Mode::ALL {
        let t = gtk::ToggleButton::with_label(m.label());
        t.set_active(m == mode);
        let model = model.clone();
        t.connect_clicked(move |t| {
            if t.is_active() {
                let model = model.clone();
                glib::idle_add_local_once(move || model.set_mode(m));
            } else {
                t.set_active(true);
            }
        });
        b.append(&t);
    }
    b.set_sensitive(idle);
    b.set_tooltip_text(Some(if idle { "Off, Focus, Break or Strict, without a timer" } else { "The timer sets the mode. Stop it to change the mode by hand." }));
    b
}

// MARK: Today

fn today(model: &Rc<Model>, tickers: &mut Tickers) -> gtk::Widget {
    let page = page_box();
    let s = model.s();
    let top = hbox(16);

    let current = vbox(8);
    current.append(&caption("Current task"));
    current.append(&styled(&s.current_task().map(|t| t.title.clone()).unwrap_or_else(|| "Nothing yet".into()), "kairos-title"));
    // Timer readout.
    let readout = vbox(4);
    if s.tempo.phase() != TempoPhase::Idle {
        let line = hbox(10);
        let big = styled(&clock(s.remaining()), "kairos-big");
        line.append(&big);
        let phase = styled(&phase_text(&s), "kairos-dim");
        phase.set_wrap(false);
        phase.set_valign(gtk::Align::End);
        phase.set_margin_bottom(10);
        line.append(&phase);
        readout.append(&line);
        let bar = gtk::ProgressBar::new();
        bar.set_fraction(s.tempo.progress(s.now));
        bar.set_size_request(320, -1);
        bar.set_halign(gtk::Align::Start);
        readout.append(&bar);
        tickers.push(Box::new(move |s| {
            big.set_text(&clock(s.remaining()));
            phase.set_text(&phase_text(s));
            bar.set_fraction(s.tempo.progress(s.now));
        }));
    } else if s.tempo.awaiting_focus() {
        readout.append(&label("\u{2615} Break over. Start the next focus session when you\u{2019}re ready."));
    } else {
        readout.append(&styled(&format!("{} \u{00B7} {}", s.active_group().map(|g| g.name.clone()).unwrap_or_default(), s.mode().label()), "kairos-dim"));
    }
    current.append(&readout);
    let buttons = hbox(8);
    let idle = s.tempo.phase() == TempoPhase::Idle;
    let main_label = if idle { "Start focus" } else if s.tempo.is_paused() { "Resume" } else { "Pause" };
    buttons.append(&suggested(button(main_label, act(model, |m| m.toggle_timer()))));
    if s.tempo.awaiting_focus() {
        let b = button("Not now", act(model, |m| m.end_break_wait()));
        b.set_tooltip_text(Some("Stop waiting. The next focus session starts a new count toward the long break."));
        buttons.append(&b);
    }
    if !idle {
        let skip = button("Skip", act(model, |m| m.request_skip()));
        skip.set_sensitive(!s.strict_locked());
        buttons.append(&skip);
        buttons.append(&button("+5 min", act(model, |m| m.add_five_minutes())));
        let stop = button("Stop", act(model, |m| m.stop_timer()));
        stop.set_sensitive(!s.strict_locked());
        buttons.append(&stop);
    }
    if let Some(t) = s.current_task() {
        let id = t.id;
        buttons.append(&button("Done", act(model, move |m| m.set_done(id, true))));
    }
    current.append(&buttons);
    let mp = mode_picker(model);
    mp.set_halign(gtk::Align::Start);
    current.append(&mp);
    if let Some(next) = s.suggestion.clone() {
        let b = gtk::Button::with_label(&format!("\u{2728} Next up: {}", next.title));
        b.add_css_class("flat");
        b.set_halign(gtk::Align::Start);
        let m = model.clone();
        b.connect_clicked(move |_| {
            let (m, t) = (m.clone(), next.clone());
            glib::idle_add_local_once(move || m.make_current(&t));
        });
        current.append(&b);
    }
    let c1 = card(&current);
    c1.set_hexpand(true);
    c1.set_size_request(-1, 130);
    top.append(&c1);

    let day = vbox(8);
    day.append(&caption("Today"));
    let today_date = Local::now().date_naive();
    day.append(&styled(&format!("{} min", s.focus_minutes(today_date, None) as i64), "kairos-title"));
    day.append(&styled("focused", "kairos-dim"));
    let blocked = s.data.block_events.iter().filter(|e| time::local(e.time).date_naive() == today_date).count();
    day.append(&styled(&format!("{blocked} blocked attempt{}", if blocked == 1 { "" } else { "s" }), "kairos-dim"));
    let c2 = card(&day);
    c2.set_size_request(200, 130);
    top.append(&c2);
    page.append(&top);

    if s.site_blocking_off() {
        let b = button("Chrome bridge settings", act(model, |m| m.set(change::NAV, |s| s.page = Page::Settings)));
        page.append(&warning(SITE_BLOCKING_OFF_TEXT, Some(b.upcast())));
    }
    if s.mode().blocks() {
        if let Some(problem) = s.jev_readiness(s.active_group()).problem() {
            page.append(&warning(&problem, None));
        }
    }
    let hidden = s.data.setup_hidden;
    drop(s);
    if !hidden {
        if let Some(card) = setup_card(model) {
            page.append(&card);
        }
    }
    let s = model.s();
    let row = hbox(10);
    row.append(&heading("Group"));
    row.append(&group_picker(model, s.data.focus.active_group_id, {
        let m = model.clone();
        move |id| m.switch_group(id)
    }));
    page.append(&row);
    page.append(&quick_add_field(model));
    page.append(&heading("Due today"));
    let now = time::now();
    let todays: Vec<TaskItem> = s
        .data
        .tasks
        .iter()
        .filter(|t| {
            if t.is_done() || t.parent_id.is_some() {
                return false;
            }
            if Some(t.id) == s.data.focus.current_task_id {
                return true;
            }
            t.due.map(|d| time::local(d).date_naive() == today_date || d < now).unwrap_or(false)
        })
        .cloned()
        .collect();
    if todays.is_empty() {
        let key = hotkeys::choice(&s.data.settings.quick_add_hotkey, hotkeys::QUICK_ADD).map(|c| c.label).unwrap_or("the quick add shortcut");
        page.append(&styled(&format!("Nothing due today. Add a task above, or press {key} from anywhere."), "kairos-dim"));
    }
    drop(s);
    for t in todays {
        page.append(&task_row(model, &t, true));
    }
    scrolled(&page).upcast()
}

fn phase_text(s: &crate::model::State) -> String {
    if s.tempo.is_paused() {
        format!("{}, paused", s.tempo.phase().label())
    } else {
        s.tempo.phase().label().to_string()
    }
}

/// The setup list on Today, until everything is done or the owner hides it.
fn setup_card(model: &Rc<Model>) -> Option<gtk::Box> {
    let list = model.s().setup_checklist();
    if list.is_complete() {
        return None;
    }
    let b = vbox(10);
    let head = hbox(8);
    let title = heading("Set up Kairos");
    title.set_wrap(false);
    head.append(&title);
    let count = styled(&format!("{} of {} done", list.done_count(), list.items.len()), "kairos-dim");
    count.set_hexpand(true);
    head.append(&count);
    let hide = button("Hide", act(model, |m| m.edit(|d| d.setup_hidden = true)));
    hide.set_tooltip_text(Some("Everything here is also in Settings."));
    head.append(&hide);
    b.append(&head);
    for item in &list.items {
        b.append(&setup_row(model, item));
    }
    Some(card(&b))
}

fn setup_row(model: &Rc<Model>, item: &SetupItem) -> gtk::Box {
    let row = hbox(10);
    let icon = gtk::Image::from_icon_name(if item.done { "emblem-ok-symbolic" } else { "radio-symbolic" });
    icon.set_valign(gtk::Align::Start);
    icon.update_property(&[gtk::accessible::Property::Label(if item.done { "Done" } else { "Not done" })]);
    row.append(&icon);
    let text = vbox(2);
    text.set_hexpand(true);
    let t = label(&item.title);
    if item.done {
        t.add_css_class("kairos-done");
    }
    text.append(&t);
    if !item.done {
        text.append(&caption(&item.detail));
    }
    row.append(&text);
    let action: Option<gtk::Widget> = match item.step {
        SetupStep::Shield | SetupStep::Read => {
            (!item.done).then(system::extensions_folder).flatten().map(|f| button("Show folder", move || system::show_folder(&f)).upcast())
        }
        SetupStep::JevKey | SetupStep::VoiceKey => (!item.done).then(|| button("Add key\u{2026}", act(model, |m| m.set(change::NAV, |s| s.page = Page::Settings))).upcast()),
        SetupStep::Scenes => (!item.done).then(|| {
            let api = model.s().data.settings.scene_backend == SceneBackend::ApiKey;
            button(if api { "Add key\u{2026}" } else { "Settings\u{2026}" }, act(model, |m| m.set(change::NAV, |s| s.page = Page::Settings))).upcast()
        }),
        SetupStep::OpenAtLogin => {
            let sw = gtk::Switch::new();
            sw.set_active(autostart::enabled());
            let m = model.clone();
            sw.connect_active_notify(move |s| {
                autostart::set(s.is_active());
                m.emit(change::KEYS);
            });
            Some(sw.upcast())
        }
        SetupStep::DockReplacement => {
            let sw = gtk::Switch::new();
            sw.set_active(model.s().data.settings.replaces_system_dock);
            let m = model.clone();
            sw.connect_active_notify(move |s| {
                let on = s.is_active();
                let m = m.clone();
                glib::idle_add_local_once(move || m.edit(|d| d.settings.set_replaces_system_dock(on)));
            });
            Some(sw.upcast())
        }
    };
    if let Some(a) = action {
        a.set_valign(gtk::Align::Center);
        row.append(&a);
    }
    row
}

// MARK: Tasks

#[derive(Default)]
struct TasksUi {
    group: Option<Id>,
    selected: Option<Id>,
    show_done: bool,
}

thread_local! {
    static TASKS_UI: RefCell<TasksUi> = RefCell::new(TasksUi::default());
}

fn tasks(model: &Rc<Model>) -> gtk::Widget {
    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    let (group, selected, show_done) = TASKS_UI.with(|u| {
        let mut u = u.borrow_mut();
        if u.group.is_none() || model.s().data.group(u.group).is_none() {
            u.group = model.s().data.focus.active_group_id;
        }
        (u.group, u.selected, u.show_done)
    });
    let left = vbox(12);
    left.set_margin_top(16);
    left.set_margin_bottom(16);
    left.set_margin_start(16);
    left.set_margin_end(16);
    left.set_size_request(380, -1);
    let head = hbox(8);
    head.append(&group_picker(model, group, {
        let m = model.clone();
        move |id| {
            TASKS_UI.with(|u| u.borrow_mut().group = Some(id));
            m.emit(change::NAV);
        }
    }));
    head.append(&spacer());
    let done_toggle = gtk::CheckButton::with_label("Show done");
    done_toggle.set_active(show_done);
    {
        let m = model.clone();
        done_toggle.connect_toggled(move |c| {
            let on = c.is_active();
            TASKS_UI.with(|u| u.borrow_mut().show_done = on);
            let m = m.clone();
            glib::idle_add_local_once(move || m.emit(change::NAV));
        });
    }
    head.append(&done_toggle);
    left.append(&head);
    left.append(&quick_add_field(model));
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::Single);
    let mut ids = vec![];
    let todo = model.s().tasks_in(group, false);
    for t in &todo {
        let row = gtk::ListBoxRow::new();
        let r = task_row(model, t, false);
        r.set_margin_start(8);
        r.set_margin_end(8);
        row.set_child(Some(&r));
        list.append(&row);
        ids.push(t.id);
    }
    if show_done {
        let done = model.s().tasks_in(group, true);
        if !done.is_empty() {
            let row = gtk::ListBoxRow::new();
            row.set_selectable(false);
            row.set_activatable(false);
            let h = heading("Done");
            h.set_margin_top(10);
            h.set_margin_start(8);
            row.set_child(Some(&h));
            list.append(&row);
            ids.push(Id::new());
        }
        for t in &done {
            let row = gtk::ListBoxRow::new();
            let r = task_row(model, t, false);
            r.set_margin_start(8);
            r.set_margin_end(8);
            row.set_child(Some(&r));
            list.append(&row);
            ids.push(t.id);
        }
    }
    if let Some(sel) = selected {
        if let Some(i) = ids.iter().position(|i| *i == sel) {
            list.select_row(list.row_at_index(i as i32).as_ref());
        }
    }
    {
        let m = model.clone();
        let ids = ids.clone();
        list.connect_row_selected(move |_, row| {
            let id = row.and_then(|r| ids.get(r.index() as usize).copied());
            let changed = TASKS_UI.with(|u| {
                let mut u = u.borrow_mut();
                let c = u.selected != id;
                u.selected = id;
                c
            });
            if changed {
                let m = m.clone();
                glib::idle_add_local_once(move || m.emit(change::NAV));
            }
        });
    }
    left.append(&scrolled(&list));
    paned.set_start_child(Some(&left));
    paned.set_shrink_start_child(false);
    let task = selected.and_then(|id| model.s().data.tasks.iter().find(|t| t.id == id).cloned());
    let right: gtk::Widget = match task {
        Some(t) => task_detail(model, &t),
        None => {
            let l = styled("Select a task", "kairos-dim");
            l.set_xalign(0.5);
            l.set_vexpand(true);
            l.upcast()
        }
    };
    right.set_size_request(280, -1);
    paned.set_end_child(Some(&right));
    paned.set_shrink_end_child(false);
    paned.upcast()
}

fn task_detail(model: &Rc<Model>, task: &TaskItem) -> gtk::Widget {
    let b = page_box();
    let id = task.id;
    let m = model.clone();
    b.append(&labeled("Title", &{
        let e = entry(&task.title, "Title", move |t| m.update_task(id, |task| task.title = t));
        e.set_hexpand(true);
        e
    }));
    let groups: Vec<(Id, String)> = model.s().data.groups.iter().map(|g| (g.id, g.name.clone())).collect();
    let names: Vec<String> = groups.iter().map(|g| g.1.clone()).collect();
    let current = task.group_id.and_then(|g| groups.iter().position(|x| x.0 == g)).unwrap_or(0);
    {
        let m = model.clone();
        let ids: Vec<Id> = groups.iter().map(|g| g.0).collect();
        b.append(&labeled("Group", &dropdown(&names, current, move |i| {
            if let Some(g) = ids.get(i).copied() {
                m.update_task(id, |t| t.group_id = Some(g));
            }
        })));
    }
    {
        let m = model.clone();
        let opts: Vec<String> = ["None", "Low", "Medium", "High"].iter().map(|s| s.to_string()).collect();
        b.append(&labeled("Priority", &dropdown(&opts, task.priority.clamp(0, 3) as usize, move |i| m.update_task(id, |t| t.priority = i as i32))));
    }
    {
        let m = model.clone();
        b.append(&switch_row("Has a due date", None, task.due.is_some(), true, move |on| {
            let m = m.clone();
            glib::idle_add_local_once(move || m.update_task(id, |t| t.due = if on { Some(time::now()) } else { None }));
        }));
    }
    if let Some(due) = task.due {
        b.append(&due_editor(model, id, due));
    }
    {
        let m = model.clone();
        let e = entry(&task.estimate_min.map(|v| v.to_string()).unwrap_or_default(), "Minutes", move |t| {
            let v = t.trim().parse::<i32>().ok().filter(|v| *v > 0);
            m.update_task(id, |task| task.estimate_min = v);
        });
        e.set_width_chars(8);
        e.set_input_purpose(gtk::InputPurpose::Digits);
        b.append(&labeled("Estimate (min)", &e));
    }
    b.append(&heading("Notes"));
    let view = gtk::TextView::new();
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.buffer().set_text(&task.notes);
    view.set_size_request(-1, 90);
    view.add_css_class("kairos-field");
    {
        let m = model.clone();
        let buf = view.buffer();
        let focus = gtk::EventControllerFocus::new();
        let original = RefCell::new(task.notes.clone());
        focus.connect_leave(move |_| {
            let t = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
            if *original.borrow() != t {
                *original.borrow_mut() = t.clone();
                m.update_task(id, |task| task.notes = t);
            }
        });
        view.add_controller(focus);
    }
    b.append(&view);
    b.append(&heading("Subtasks"));
    for sub in model.s().subtasks(id) {
        let c = gtk::CheckButton::with_label(&sub.title);
        c.set_active(sub.is_done());
        let (m, sid) = (model.clone(), sub.id);
        c.connect_toggled(move |c| {
            let (m, on) = (m.clone(), c.is_active());
            glib::idle_add_local_once(move || m.set_done(sid, on));
        });
        b.append(&c);
    }
    let sub = gtk::Entry::new();
    sub.set_placeholder_text(Some("Add a subtask"));
    {
        let (m, t) = (model.clone(), task.clone());
        sub.connect_activate(move |e| {
            let text = e.text().to_string();
            e.set_text("");
            m.add_subtask(&text, &t);
        });
    }
    b.append(&sub);
    if task.listened_sec > 0 {
        b.append(&labeled("Listened with Kairos Read", &gtk::Label::new(Some(&format!("{} min", task.listened_sec / 60)))));
    }
    scrolled(&b).upcast()
}

fn due_editor(model: &Rc<Model>, id: Id, due: time::Date) -> gtk::Box {
    let local = time::local(due);
    let row = hbox(8);
    let cal_btn = gtk::MenuButton::new();
    cal_btn.set_label(&local.format("%a %-d %b %Y").to_string());
    let cal = gtk::Calendar::new();
    if let Ok(dt) = glib::DateTime::from_local(local.year(), local.month() as i32, local.day() as i32, 0, 0, 0.0) {
        cal.select_day(&dt);
    }
    let pop = gtk::Popover::new();
    pop.set_child(Some(&cal));
    cal_btn.set_popover(Some(&pop));
    let hour = gtk::SpinButton::with_range(0.0, 23.0, 1.0);
    hour.set_value(local.hour() as f64);
    hour.set_wrap(true);
    let minute = gtk::SpinButton::with_range(0.0, 59.0, 5.0);
    minute.set_value(local.minute() as f64);
    minute.set_wrap(true);
    let apply = {
        let (m, cal, hour, minute) = (model.clone(), cal.clone(), hour.clone(), minute.clone());
        Rc::new(move || {
            let d = cal.date();
            let when = Local.with_ymd_and_hms(d.year(), d.month() as u32, d.day_of_month() as u32, hour.value() as u32, minute.value() as u32, 0).single();
            if let Some(w) = when {
                let utc = w.with_timezone(&chrono::Utc);
                if Some(utc) != m.s().data.tasks.iter().find(|t| t.id == id).and_then(|t| t.due) {
                    m.update_task(id, |t| t.due = Some(utc));
                }
            }
        })
    };
    {
        let (a, p) = (apply.clone(), pop.clone());
        cal.connect_day_selected(move |_| {
            p.popdown();
            let a = a.clone();
            glib::idle_add_local_once(move || a());
        });
    }
    for spin in [&hour, &minute] {
        let a = apply.clone();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(move |_| a());
        spin.add_controller(focus);
        let a = apply.clone();
        spin.connect_activate(move |_| a());
    }
    row.append(&label("Due"));
    row.append(&spacer());
    row.append(&cal_btn);
    row.append(&hour);
    row.append(&gtk::Label::new(Some(":")));
    row.append(&minute);
    row
}

use chrono::Timelike;

// MARK: Groups and Rules

fn group_sidebar(model: &Rc<Model>, allow_edit: bool) -> gtk::Box {
    let b = vbox(0);
    b.set_size_request(190, -1);
    {
        let s = model.s();
        if s.data.group(s.selected_group_id).is_none() {
            let active = s.data.focus.active_group_id;
            drop(s);
            model.state.borrow_mut().selected_group_id = active;
        }
    }
    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    list.set_vexpand(true);
    let s = model.s();
    let ids: Vec<Id> = s.data.groups.iter().map(|g| g.id).collect();
    for (i, g) in s.data.groups.iter().enumerate() {
        let row = gtk::ListBoxRow::new();
        let r = hbox(8);
        r.set_margin_top(6);
        r.set_margin_bottom(6);
        r.set_margin_start(8);
        let img = gtk::Image::from_icon_name(icons::group_icon(&g.icon));
        let (red, green, blue) = icons::rgb(&g.color_hex);
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&format!("image {{ color: rgb({},{},{}); }}", (red * 255.0) as u8, (green * 255.0) as u8, (blue * 255.0) as u8));
        #[allow(deprecated)]
        img.style_context().add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        r.append(&img);
        let name = gtk::Label::new(Some(&g.name));
        name.set_hexpand(true);
        name.set_xalign(0.0);
        r.append(&name);
        if allow_edit {
            let n = s.data.groups.len();
            let (m1, id1) = (model.clone(), g.id);
            let up = icon_button("go-up-symbolic", &format!("Move {} up", g.name), act(&m1, move |m| m.edit(|d| {
                if let Some(i) = d.groups.iter().position(|x| x.id == id1) {
                    if i > 0 {
                        d.groups.swap(i, i - 1);
                    }
                }
            })));
            up.set_sensitive(i > 0);
            r.append(&up);
            let (m2, id2) = (model.clone(), g.id);
            let down = icon_button("go-down-symbolic", &format!("Move {} down", g.name), act(&m2, move |m| m.edit(|d| {
                if let Some(i) = d.groups.iter().position(|x| x.id == id2) {
                    if i + 1 < d.groups.len() {
                        d.groups.swap(i, i + 1);
                    }
                }
            })));
            down.set_sensitive(i + 1 < n);
            r.append(&down);
        }
        row.set_child(Some(&r));
        list.append(&row);
        if Some(g.id) == s.selected_group_id {
            list.select_row(Some(&row));
        }
    }
    drop(s);
    {
        let m = model.clone();
        list.connect_row_selected(move |_, row| {
            let id = row.and_then(|r| ids.get(r.index() as usize).copied());
            if id.is_some() && id != m.s().selected_group_id {
                let m = m.clone();
                glib::idle_add_local_once(move || m.set(change::NAV, |s| s.selected_group_id = id));
            }
        });
    }
    b.append(&list);
    if allow_edit {
        b.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let add = gtk::Button::new();
        add.add_css_class("flat");
        let inner = hbox(8);
        inner.append(&gtk::Image::from_icon_name("list-add-symbolic"));
        inner.append(&gtk::Label::new(Some("New Group")));
        add.set_child(Some(&inner));
        add.set_tooltip_text(Some("Add a group (Shift+Ctrl+N)"));
        add.set_margin_top(6);
        add.set_margin_bottom(6);
        add.set_margin_start(6);
        add.set_margin_end(6);
        add.connect_clicked(clicked(act(model, |m| {
            let id = m.add_group("New group");
            m.set(change::NAV, |s| {
                s.selected_group_id = Some(id);
                s.rename_group_id = Some(id);
            });
        })));
        b.append(&add);
    }
    b
}

fn groups(model: &Rc<Model>) -> gtk::Widget {
    let root = hbox(0);
    root.append(&group_sidebar(model, true));
    root.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    let selected = model.s().selected_group_id.and_then(|id| model.s().data.group(Some(id)).cloned());
    match selected {
        Some(g) => root.append(&group_detail(model, &g)),
        None => root.append(&{
            let l = styled("Select a group", "kairos-dim");
            l.set_hexpand(true);
            l.set_xalign(0.5);
            l
        }),
    }
    // Shift+Ctrl+N adds a group.
    let keys = gtk::ShortcutController::new();
    let m = model.clone();
    keys.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("<Shift><Control>n"),
        Some(gtk::CallbackAction::new(move |_, _| {
            let id = m.add_group("New group");
            m.set(change::NAV, |s| {
                s.selected_group_id = Some(id);
                s.rename_group_id = Some(id);
            });
            glib::Propagation::Stop
        })),
    ));
    keys.set_scope(gtk::ShortcutScope::Global);
    root.add_controller(keys);
    root.upcast()
}

fn group_detail(model: &Rc<Model>, g: &AppGroup) -> gtk::Widget {
    let b = page_box();
    b.set_hexpand(true);
    let id = g.id;
    let s = model.s();
    let index = s.data.groups.iter().position(|x| x.id == id).unwrap_or(0);
    let (form_outer, form) = section("Group");
    let name = {
        let m = model.clone();
        entry(&g.name, "Name", move |t| m.rename_group(id, &t))
    };
    name.set_hexpand(true);
    form.append(&labeled("Name", &name));
    let presets: Vec<String> = s.data.settings.presets.iter().map(|p| format!("{} ({}/{})", p.name, p.focus_min, p.short_break_min)).collect();
    let preset_names: Vec<String> = s.data.settings.presets.iter().map(|p| p.name.clone()).collect();
    let current = preset_names.iter().position(|n| *n == g.tempo_preset).unwrap_or(0);
    {
        let m = model.clone();
        form.append(&labeled("Timer preset", &dropdown(&presets, current, move |i| {
            if let Some(n) = preset_names.get(i).cloned() {
                m.update_group(id, |g| g.tempo_preset = n);
            }
        })));
    }
    let shortcut = if index < 9 && s.data.settings.group_hotkeys { format!("Ctrl+{}", index + 1) } else { "None".into() };
    form.append(&labeled("Shortcut", &gtk::Label::new(Some(&shortcut))));
    let desktops: Vec<String> = std::iter::once("Don't switch".to_string()).chain((1..=9).map(|n| format!("Workspace {n}"))).collect();
    {
        let m = model.clone();
        form.append(&labeled("Switch to workspace", &dropdown(&desktops, g.desktop.clamp(0, 9) as usize, move |i| m.update_group(id, |g| g.desktop = i as i32))));
    }
    if g.desktop > 0 {
        form.append(&caption(&format!("Switching to {} also moves to workspace {}. Add workspaces in System Settings > Workspaces if there are fewer.", g.name, g.desktop)));
    }
    {
        // Index 0 follows Settings; the rest are SwitchBehavior::ALL in order.
        let default = s.data.settings.switch_behavior.label();
        let options: Vec<String> = std::iter::once(format!("As in Settings ({})", default.to_lowercase()))
            .chain(SwitchBehavior::ALL.iter().map(|b| b.label().to_string()))
            .collect();
        let cur = g.switch_behavior.and_then(|b| SwitchBehavior::ALL.iter().position(|x| *x == b)).map_or(0, |i| i + 1);
        let m = model.clone();
        form.append(&labeled("Other apps", &dropdown(&options, cur, move |i| {
            let next = if i == 0 { None } else { SwitchBehavior::ALL.get(i - 1).copied() };
            m.update_group(id, |g| g.switch_behavior = next);
        })));
        form.append(&caption(&format!("What switching to {} does to the windows of other apps.", g.name)));
    }
    b.append(&form_outer);

    let look = vbox(6);
    look.append(&heading("Colour"));
    let swatches = hbox(6);
    for (hex, cname) in icons::COLORS {
        let btn = gtk::Button::new();
        btn.add_css_class("kairos-swatch");
        if g.color_hex.eq_ignore_ascii_case(hex) {
            btn.add_css_class("chosen");
        }
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&format!("button {{ background: {hex}; }}"));
        #[allow(deprecated)]
        btn.style_context().add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        btn.set_tooltip_text(Some(cname));
        btn.update_property(&[gtk::accessible::Property::Label(cname)]);
        let hex = hex.to_string();
        btn.connect_clicked(clicked(act(model, move |m| {
            let h = hex.clone();
            m.update_group(id, |g| g.color_hex = h);
        })));
        swatches.append(&btn);
    }
    look.append(&swatches);
    let ih = heading("Icon");
    ih.set_margin_top(6);
    look.append(&ih);
    let choices = hbox(4);
    for (sym, fd, iname) in icons::GROUP_ICONS {
        let btn = gtk::Button::from_icon_name(fd);
        btn.add_css_class("flat");
        btn.add_css_class("kairos-icon-choice");
        if g.icon == *sym {
            btn.add_css_class("chosen");
        }
        btn.set_tooltip_text(Some(iname));
        btn.update_property(&[gtk::accessible::Property::Label(iname)]);
        let sym = sym.to_string();
        btn.connect_clicked(clicked(act(model, move |m| {
            let s2 = sym.clone();
            m.update_group(id, |g| g.icon = s2);
        })));
        choices.append(&btn);
    }
    look.append(&choices);
    b.append(&look);
    drop(s);
    {
        let m = model.clone();
        b.append(&app_list(model, "Apps in Kairos Bar", "No apps yet. Add some, or drag an app onto Kairos Bar.", &g.dock_apps, true, true, move |next| {
            m.update_group(id, |g| g.dock_apps = next)
        }));
    }
    b.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let del = button("Delete Group\u{2026}", {
        let m = model.clone();
        move || actions::delete(&m, id)
    });
    del.add_css_class("destructive-action");
    del.set_halign(gtk::Align::Start);
    del.set_sensitive(model.s().data.groups.len() > 1);
    b.append(&del);
    // A new or renamed group's name takes focus once it shows.
    if model.s().rename_group_id == Some(id) {
        let (m, name) = (model.clone(), name.clone());
        glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
            name.grab_focus();
            m.state.borrow_mut().rename_group_id = None;
        });
    }
    scrolled(&b).upcast()
}

fn rules_page(model: &Rc<Model>) -> gtk::Widget {
    let root = hbox(0);
    root.append(&group_sidebar(model, false));
    root.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    let selected = model.s().selected_group_id.and_then(|id| model.s().data.group(Some(id)).cloned());
    let Some(g) = selected else {
        let l = styled("Select a group", "kairos-dim");
        l.set_hexpand(true);
        root.append(&l);
        return root.upcast();
    };
    let id = g.id;
    let lock = model.s().rule_lock(id);
    let locked = lock.is_some();
    let b = page_box();
    b.set_hexpand(true);
    b.append(&styled(&format!("These apply while {} is the active group and the mode is Focus or Strict.", g.name), "kairos-dim"));
    if let Some(l) = &lock {
        let row = hbox(8);
        let icon = gtk::Image::from_icon_name("changes-prevent-symbolic");
        icon.add_css_class("kairos-lock");
        row.append(&icon);
        row.append(&styled(l, "kairos-lock"));
        b.append(&row);
    }
    {
        let m = model.clone();
        b.append(&app_list(model, "Blocked apps", "No apps blocked. Add other browsers here to stop site rules being bypassed.", &g.blocked_apps, false, !locked, move |next| {
            m.update_group(id, |g| g.blocked_apps = next)
        }));
    }
    {
        let m = model.clone();
        b.append(&string_list("Blocked sites", "example.com", &g.block_sites, normalize_site, true, !locked, move |next| m.update_group(id, |g| g.block_sites = next)));
    }
    {
        let m = model.clone();
        b.append(&string_list("Always allowed sites", "docs.google.com", &g.allow_sites, normalize_site, !locked, true, move |next| m.update_group(id, |g| g.allow_sites = next)));
    }
    // Hidden page elements.
    let hidden = vbox(6);
    hidden.append(&heading("Hidden page elements"));
    if g.hidden_elements.is_empty() {
        hidden.append(&caption("None yet. Use \u{201C}Hide an element on this page\u{201D} in the Kairos Shield popup in Chrome."));
    }
    for (host, selectors) in &g.hidden_elements {
        for sel in selectors {
            let row = hbox(8);
            row.append(&styled(host, "kairos-heading"));
            let l = styled(sel, "kairos-dim");
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            l.set_wrap(false);
            l.set_hexpand(true);
            row.append(&l);
            if !locked {
                let (m, host, sel) = (model.clone(), host.clone(), sel.clone());
                row.append(&icon_button("list-remove-symbolic", &format!("Remove rule for {host}"), move || {
                    let (h, s2) = (host.clone(), sel.clone());
                    m.update_group(id, move |g| g.show_element(&s2, &h));
                }));
            }
            hidden.append(&row);
        }
    }
    b.append(&hidden);
    // Jev.
    let jev = vbox(6);
    jev.append(&heading("Smart blocking with Jev"));
    jev.append(&caption("Jev is a service from TypeSafe that checks whether a page or app fits your current task. It needs a TypeSafe key (Settings > API keys)."));
    {
        let m = model.clone();
        jev.append(&switch_row("Ask Jev about sites and apps that are on no list", None, g.jev_enabled, !(locked && g.jev_enabled), move |on| {
            let m = m.clone();
            glib::idle_add_local_once(move || m.update_group(id, |g| g.jev_enabled = on));
        }));
    }
    let row = hbox(8);
    row.append(&label("Block when \u{201C}distracting\u{201D} is at least"));
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.5, 0.95, 0.05);
    scale.set_value(g.jev_threshold);
    scale.set_size_request(160, -1);
    scale.set_draw_value(false);
    let value = gtk::Label::new(Some(&format!("{:.2}", g.jev_threshold)));
    {
        let (m, value) = (model.clone(), value.clone());
        scale.connect_value_changed(move |s| {
            let v = (s.value() * 20.0).round() / 20.0;
            value.set_text(&format!("{v:.2}"));
            m.update_group(id, |g| g.jev_threshold = v);
        });
    }
    row.append(&scale);
    row.append(&value);
    row.set_sensitive(g.jev_enabled && !locked);
    jev.append(&row);
    if let Some(problem) = model.s().jev_readiness(Some(&g)).problem() {
        jev.append(&styled(&format!("\u{26A0} {problem}"), "kairos-warning-text"));
    }
    b.append(&jev);
    root.append(&scrolled(&b));
    root.upcast()
}

// MARK: Stats

fn stats(model: &Rc<Model>) -> gtk::Widget {
    let b = page_box();
    let s = model.s();
    let today = Local::now().date_naive();
    let days: Vec<chrono::NaiveDate> = (0..7).rev().map(|i| today - ChronoDuration::days(i)).collect();
    let first = Local.from_local_datetime(&days[0].and_hms_opt(0, 0, 0).unwrap()).single().map(|d| d.with_timezone(&chrono::Utc)).unwrap_or_else(time::now);
    let week: Vec<&Session> = s.data.sessions.iter().filter(|x| x.kind == SessionKind::Focus && x.start >= first).collect();
    b.append(&heading("Last 7 days"));
    let tiles = hbox(16);
    tiles.set_homogeneous(true);
    let streak = {
        let mut day = today;
        let mut count = 0;
        // Today only counts once it has a session; a missing today does not break the streak.
        if s.focus_minutes(day, None) == 0.0 {
            day -= ChronoDuration::days(1);
        }
        while s.focus_minutes(day, None) > 0.0 && count < 3650 {
            count += 1;
            day -= ChronoDuration::days(1);
        }
        count
    };
    for (value, text) in [
        (format!("{}", week.iter().map(|x| x.minutes()).sum::<f64>() as i64), "minutes focused"),
        (format!("{}", week.iter().filter(|x| x.completed).count()), "sessions completed"),
        (format!("{}", week.iter().map(|x| x.interruptions).sum::<i32>()), "interruptions"),
        (format!("{streak}"), "day streak"),
    ] {
        let t = vbox(2);
        t.add_css_class("kairos-stat");
        t.append(&styled(&value, "kairos-stat-value"));
        t.append(&caption(text));
        tiles.append(&t);
    }
    b.append(&tiles);
    if s.data.unassigned_listened_sec >= 60 {
        b.append(&styled(&format!("Listened with Kairos Read without a current task: {} min", s.data.unassigned_listened_sec / 60), "kairos-dim"));
    }
    b.append(&heading("Focus minutes by day"));
    // Stacked bars per day, by group.
    let groups: Vec<(String, (f64, f64, f64), Id)> = s.data.groups.iter().map(|g| (g.name.clone(), icons::rgb(&g.color_hex), g.id)).collect();
    let values: Vec<Vec<f64>> = days.iter().map(|d| groups.iter().map(|g| s.focus_minutes(*d, Some(g.2))).collect()).collect();
    let labels: Vec<String> = days.iter().map(|d| d.format("%a").to_string()).collect();
    let chart = gtk::DrawingArea::new();
    chart.set_content_height(220);
    chart.set_hexpand(true);
    let gcolors: Vec<(f64, f64, f64)> = groups.iter().map(|g| g.1).collect();
    chart.set_draw_func(move |area, cr, w, h| {
        let fg = area.color();
        let (w, h) = (w as f64, h as f64);
        let left = 36.0;
        let bottom = 22.0;
        let plot_h = h - bottom - 8.0;
        let max = values.iter().map(|v| v.iter().sum::<f64>()).fold(0.0, f64::max).max(30.0);
        let step = nice_step(max);
        let top = (max / step).ceil() * step;
        cr.set_font_size(11.0);
        let mut y = 0.0;
        while y <= top + 0.1 {
            let py = 8.0 + plot_h - plot_h * y / top;
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.12);
            cr.rectangle(left, py, w - left, 1.0);
            let _ = cr.fill();
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.6);
            cr.move_to(4.0, py + 4.0);
            let _ = cr.show_text(&format!("{}", y as i64));
            y += step;
        }
        let slot = (w - left) / values.len() as f64;
        let bar_w = (slot * 0.55).min(48.0);
        for (i, day) in values.iter().enumerate() {
            let x = left + slot * i as f64 + (slot - bar_w) / 2.0;
            let mut acc = 0.0;
            for (j, v) in day.iter().enumerate() {
                if *v <= 0.0 {
                    continue;
                }
                let y0 = 8.0 + plot_h - plot_h * (acc + v) / top;
                let hh = plot_h * v / top;
                let (r, g, b) = gcolors[j];
                cr.set_source_rgb(r, g, b);
                cr.rectangle(x, y0, bar_w, hh);
                let _ = cr.fill();
                acc += v;
            }
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.7);
            let ext = cr.text_extents(&labels[i]).ok();
            let tw = ext.map(|e| e.width()).unwrap_or(20.0);
            cr.move_to(left + slot * i as f64 + (slot - tw) / 2.0, h - 6.0);
            let _ = cr.show_text(&labels[i]);
        }
    });
    b.append(&chart);
    let legend = hbox(14);
    for (name, (r, g, bl), _) in &groups {
        let item = hbox(6);
        let dot = gtk::DrawingArea::new();
        dot.set_content_width(10);
        dot.set_content_height(10);
        let (r, g, bl) = (*r, *g, *bl);
        dot.set_draw_func(move |_, cr, w, h| {
            cr.set_source_rgb(r, g, bl);
            cr.arc(w as f64 / 2.0, h as f64 / 2.0, 5.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        });
        dot.set_valign(gtk::Align::Center);
        item.append(&dot);
        item.append(&caption(name));
        legend.append(&item);
    }
    b.append(&legend);
    b.append(&heading("Most blocked"));
    let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for e in s.data.block_events.iter().filter(|e| e.time >= first) {
        let c = counts.entry(e.display_name().to_string()).or_default();
        c.0 += 1;
        if e.overridden {
            c.1 += 1;
        }
    }
    let mut top: Vec<(String, (usize, usize))> = counts.into_iter().collect();
    top.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    if top.is_empty() {
        b.append(&styled("Nothing has been blocked yet.", "kairos-dim"));
    }
    for (target, (count, allowed)) in top.into_iter().take(8) {
        let row = hbox(8);
        let l = label(&target);
        l.set_hexpand(true);
        row.append(&l);
        row.append(&styled(&format!("{count} blocked{}", if allowed > 0 { format!(", {allowed} allowed") } else { String::new() }), "kairos-dim"));
        b.append(&row);
    }
    scrolled(&b).upcast()
}

fn nice_step(max: f64) -> f64 {
    for s in [10.0, 15.0, 30.0, 60.0, 120.0, 240.0, 480.0] {
        if max / s <= 6.0 {
            return s;
        }
    }
    960.0
}

// MARK: Settings

fn settings(model: &Rc<Model>) -> gtk::Widget {
    let b = page_box();
    let s = model.s();
    let st = s.data.settings.clone();

    // General
    let (outer, sec) = section("General");
    sec.append(&switch_row("Open Kairos at login", None, autostart::enabled(), !kairos_core::store::is_scratch(), {
        let m = model.clone();
        move |on| {
            autostart::set(on);
            m.emit(change::KEYS);
        }
    }));
    sec.append(&switch_row("Notify me when a task is due", None, st.notifies_when_due, true, setter(model, |d, on| d.settings.notifies_when_due = on)));
    let behaviors: Vec<String> = SwitchBehavior::ALL.iter().map(|b| b.label().to_string()).collect();
    {
        let m = model.clone();
        let cur = SwitchBehavior::ALL.iter().position(|b| *b == st.switch_behavior).unwrap_or(0);
        sec.append(&labeled("When switching group", &dropdown(&behaviors, cur, move |i| {
            let m = m.clone();
            glib::idle_add_local_once(move || m.edit(|d| d.settings.switch_behavior = SwitchBehavior::ALL[i]));
        })));
    }
    match st.switch_behavior {
        SwitchBehavior::Hide => sec.append(&caption("Kairos Bar shows the active group\u{2019}s apps. Switching groups minimises other apps\u{2019} windows and brings back the windows Kairos minimised for apps in this group. The file manager stays available.")),
        SwitchBehavior::HideBlocked => sec.append(&caption("Switching to a group minimises its blocked apps, even with the timer off. Choose them under Rules > Blocked apps. Kairos restores their windows when you switch to a group that allows them.")),
        SwitchBehavior::Leave => {}
    }
    sec.append(&caption("A group can choose differently under Groups > Other apps."));
    sec.append(&hotkey_picker(model, "Quick add", hotkeys::QUICK_ADD, &st.quick_add_hotkey, |d, v| d.settings.quick_add_hotkey = v));
    sec.append(&hotkey_picker(model, "Start or pause the timer", hotkeys::TIMER, &st.timer_hotkey, |d, v| d.settings.timer_hotkey = v));
    sec.append(&switch_row(
        "Ctrl+1 to Ctrl+9 switch groups",
        Some("Cinnamon can use these to switch workspaces. While this is on, Kairos takes them."),
        st.group_hotkeys,
        true,
        setter(model, |d, on| d.settings.group_hotkeys = on),
    ));
    if let Some(f) = HotkeyPlan::failure_text(&s.hotkey_failures) {
        sec.append(&styled(&format!("\u{26A0} {f}"), "kairos-warning-text"));
    }
    b.append(&outer);

    // Kairos Bar
    let (outer, sec) = section("Kairos Bar");
    sec.append(&switch_row("Show Kairos Bar", None, st.dock_visible, true, setter(model, |d, on| d.settings.dock_visible = on)));
    {
        let m = model.clone();
        let edges: Vec<String> = DockEdge::ALL.iter().map(|e| e.label().to_string()).collect();
        let cur = DockEdge::ALL.iter().position(|e| *e == st.dock_edge).unwrap_or(0);
        sec.append(&labeled("Screen edge", &dropdown(&edges, cur, move |i| {
            let m = m.clone();
            glib::idle_add_local_once(move || m.edit(|d| d.settings.dock_edge = DockEdge::ALL[i]));
        })));
    }
    {
        let m = model.clone();
        let looks: Vec<String> = BarAppearance::ALL.iter().map(|e| e.label().to_string()).collect();
        let cur = BarAppearance::ALL.iter().position(|e| *e == st.bar_appearance).unwrap_or(0);
        sec.append(&labeled("Appearance", &dropdown(&looks, cur, move |i| {
            let m = m.clone();
            glib::idle_add_local_once(move || m.edit(|d| d.settings.bar_appearance = BarAppearance::ALL[i]));
        })));
    }
    {
        let row = hbox(10);
        let l = label(&format!("Icon size: {}", st.icon_size as i64));
        l.set_wrap(false);
        l.set_width_chars(13);
        row.append(&l);
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 20.0, 64.0, 2.0);
        scale.set_value(st.icon_size);
        scale.set_hexpand(true);
        scale.set_draw_value(false);
        let m = model.clone();
        scale.connect_value_changed(move |sc| {
            let v = (sc.value() / 2.0).round() * 2.0;
            l.set_text(&format!("Icon size: {}", v as i64));
            if v != m.s().data.settings.icon_size {
                m.edit(|d| d.settings.icon_size = v);
            }
        });
        row.append(&scale);
        sec.append(&row);
        sec.append(&caption("You can also drag the divider on Kairos Bar itself."));
    }
    sec.append(&switch_row("Automatically hide and show Kairos Bar", None, st.auto_hide_bar, true, setter(model, |d, on| d.settings.auto_hide_bar = on)));
    sec.append(&switch_row(
        "Hide Kairos Bar in full-screen apps",
        Some(if st.auto_hide_bar {
            "With auto-hide on, Kairos Bar always keeps out of full-screen apps."
        } else {
            "Kairos Bar stays on every workspace and keeps out of full-screen apps. Rest the pointer on the screen edge to bring it out there."
        }),
        st.auto_hide_bar || st.hides_in_full_screen,
        !st.auto_hide_bar,
        setter(model, |d, on| d.settings.hides_in_full_screen = on),
    ));
    sec.append(&switch_row(
        "Keep windows clear of Kairos Bar",
        Some(if st.auto_hide_bar {
            "Only while Kairos Bar stays on screen. With auto-hide on, windows can go under it, as with a hidden panel."
        } else {
            "Kairos Bar reserves its edge, so maximised windows stop at it, and windows that would sit under it are moved or shortened. Full-screen windows are left alone."
        }),
        st.keeps_windows_clear,
        true,
        setter(model, |d, on| d.settings.keeps_windows_clear = on),
    ));
    sec.append(&switch_row("Show Downloads and Trash at the end of Kairos Bar", None, st.shows_dock_folders, true, setter(model, |d, on| d.settings.shows_dock_folders = on)));
    sec.append(&switch_row(
        "Use Kairos Bar instead of Cinnamon\u{2019}s panel",
        Some("Kairos sets Cinnamon\u{2019}s panels to hide themselves and puts your panel settings back when you turn this off or quit. Turning this on also turns on \u{201C}Automatically hide and show Kairos Bar\u{201D}. If Kairos ever stops without putting the panel back, a small background helper does it within 30 seconds."),
        st.replaces_system_dock,
        true,
        setter(model, |d, on| d.settings.set_replaces_system_dock(on)),
    ));
    b.append(&outer);

    // Timer
    let (outer, sec) = section("Timer");
    for (i, p) in st.presets.iter().enumerate() {
        let row = hbox(10);
        let n = label(&p.name);
        n.set_size_request(90, -1);
        row.append(&n);
        let m1 = model.clone();
        row.append(&labeled_spin("Focus", 5.0, 180.0, 5.0, p.focus_min as f64, move |v| m1.edit(|d| d.settings.presets[i].focus_min = v as i32)));
        let m2 = model.clone();
        row.append(&labeled_spin("Break", 1.0, 60.0, 1.0, p.short_break_min as f64, move |v| m2.edit(|d| d.settings.presets[i].short_break_min = v as i32)));
        let m3 = model.clone();
        row.append(&labeled_spin("Long", 5.0, 90.0, 5.0, p.long_break_min as f64, move |v| m3.edit(|d| d.settings.presets[i].long_break_min = v as i32)));
        let strict = gtk::CheckButton::with_label("Strict");
        strict.set_active(p.strict);
        let m4 = model.clone();
        strict.connect_toggled(move |c| {
            let on = c.is_active();
            m4.edit(|d| d.settings.presets[i].strict = on);
        });
        row.append(&strict);
        sec.append(&row);
    }
    sec.append(&switch_row("Show a full-screen break reminder", None, st.break_overlay, true, setter(model, |d, on| d.settings.break_overlay = on)));
    sec.append(&switch_row(
        "Start the next focus session automatically",
        Some("Off, the timer waits when a break ends and notifies you. Start the next session from Today, the tray icon or the timer shortcut."),
        st.starts_focus_after_break,
        true,
        setter(model, |d, on| d.settings.starts_focus_after_break = on),
    ));
    b.append(&outer);

    // Blocking
    let (outer, sec) = section("Blocking");
    {
        let m = model.clone();
        sec.append(&labeled("Overrides per session", &spin(0.0, 10.0, 1.0, st.max_overrides_per_session as f64, move |v| m.edit(|d| d.settings.max_overrides_per_session = v as i32))));
        let m = model.clone();
        sec.append(&labeled("Each override lasts (min)", &spin(1.0, 30.0, 1.0, st.override_minutes as f64, move |v| m.edit(|d| d.settings.override_minutes = v as i32))));
    }
    sec.append(&switch_row("Block the page when Jev is slow or offline", None, st.jev_fail_closed, true, setter(model, |d, on| d.settings.jev_fail_closed = on)));
    sec.append(&switch_row(
        "Block browsers that Kairos Shield is not running in",
        Some("In Focus and Strict, Firefox and other browsers that can\u{2019}t run Kairos Shield are minimised, and so are Chromium browsers (Chrome, Brave, Edge, Vivaldi) while no Kairos Shield is connected. Kairos can\u{2019}t tell which Chromium browser Shield is running in, so one connected Shield counts for all of them."),
        st.block_unshielded_browsers,
        true,
        setter(model, |d, on| d.settings.block_unshielded_browsers = on),
    ));
    if let Some(e) = &s.last_jev_error {
        sec.append(&styled(&format!("Last Jev error: {e}"), "kairos-warning-text"));
    }
    b.append(&outer);

    // Keys
    let (outer, sec) = section("API keys (stored in your login keyring)");
    sec.append(&key_row(model, "TypeSafe (Jev)", s.has_jev_key, "typesafe"));
    sec.append(&key_row(model, "Anthropic (3D scenes)", s.has_claude_key, "anthropic"));
    b.append(&outer);

    // Scenes
    let (outer, sec) = section("3D scenes (Kairos Read)");
    {
        let m = model.clone();
        let opts: Vec<String> = SceneBackend::ALL.iter().map(|e| e.label().to_string()).collect();
        let cur = SceneBackend::ALL.iter().position(|e| *e == st.scene_backend).unwrap_or(0);
        sec.append(&labeled("Write scenes with", &dropdown(&opts, cur, move |i| {
            let m = m.clone();
            glib::idle_add_local_once(move || m.edit(|d| d.settings.scene_backend = SceneBackend::ALL[i]));
        })));
    }
    let (ready, text) = s.scene_setup.status(st.scene_backend, s.has_claude_key);
    sec.append(&styled(&text, if ready { "kairos-dim" } else { "kairos-warning-text" }));
    b.append(&outer);

    // Voice
    let (outer, sec) = section("Reading voice (Kairos Read)");
    {
        let m = model.clone();
        let opts: Vec<String> = VoiceProvider::ALL.iter().map(|e| e.label().to_string()).collect();
        let cur = VoiceProvider::ALL.iter().position(|e| *e == st.voice_provider).unwrap_or(0);
        sec.append(&labeled("Voice", &dropdown(&opts, cur, move |i| {
            let m = m.clone();
            glib::idle_add_local_once(move || {
                m.edit(|d| d.settings.set_voice_provider(VoiceProvider::ALL[i]));
                m.load_cloud_voices();
            });
        })));
    }
    match st.voice_provider {
        VoiceProvider::Elevenlabs => sec.append(&key_row(model, "ElevenLabs API key", s.voice_keys.contains("elevenlabs"), "elevenlabs")),
        VoiceProvider::Openai => sec.append(&key_row(model, "OpenAI API key", s.voice_keys.contains("openai"), "openai")),
        VoiceProvider::System => {}
    }
    if st.voice_provider != VoiceProvider::System {
        let mut names = vec!["Default".to_string()];
        names.extend(s.cloud_voices.iter().map(|v| v.name.clone()));
        let mut ids = vec![String::new()];
        ids.extend(s.cloud_voices.iter().map(|v| v.id.clone()));
        let cur = ids.iter().position(|i| *i == st.voice_id).unwrap_or(0);
        let m = model.clone();
        sec.append(&labeled("Voice name", &dropdown(&names, cur, move |i| {
            let v = ids.get(i).cloned().unwrap_or_default();
            m.edit(|d| d.settings.voice_id = v);
        })));
        let label = st.voice_provider.label();
        sec.append(&caption(&if s.cloud_voice_ready() {
            format!("Pick \u{201C}{label}\u{201D} in the Kairos Read voice menu. Audio plays from this computer, and each sentence is sent to {label}.")
        } else {
            "Add the API key to use this voice.".into()
        }));
        if let Some(e) = &s.last_voice_error {
            sec.append(&styled(e, "kairos-warning-text"));
        }
    }
    b.append(&outer);

    // Bridge
    let (outer, sec) = section("Chrome bridge");
    let browsers = bridge_installer::installed_browsers();
    sec.append(&labeled("Installed for", &gtk::Label::new(Some(&if browsers.is_empty() { "No browser yet".into() } else { browsers.join(", ") }))));
    let connected = if s.bridge_clients.is_empty() {
        "Nothing".to_string()
    } else {
        s.bridge_clients.iter().map(|c| format!("Kairos {}", crate::model::capitalize(c))).collect::<Vec<_>>().join(", ")
    };
    sec.append(&labeled("Connected now", &gtk::Label::new(Some(&connected))));
    let row = hbox(8);
    {
        let m = model.clone();
        row.append(&button("Install bridge", move || {
            let done = bridge_installer::install();
            if kairos_core::store::is_scratch() {
                alert(main_window(&m).as_ref(), "Test profile", "KAIROS_HOME is set, so the real browser manifests were left alone.");
            } else if done.is_empty() {
                alert(main_window(&m).as_ref(), "No browser found", "No Chromium browser profile was found in ~/.config.");
            }
            m.emit(change::CLIENTS);
        }));
    }
    if let Some(folder) = system::extensions_folder() {
        row.append(&button("Show extensions folder", move || system::show_folder(&folder)));
    }
    sec.append(&row);
    sec.append(&caption("In Chrome or Brave open chrome://extensions, turn on Developer mode, choose \u{201C}Load unpacked\u{201D}, and pick the shield and read folders."));
    b.append(&outer);

    // Privacy
    let (outer, sec) = section("Privacy");
    {
        let m = model.clone();
        sec.append(&string_list("Never send off this computer", "mybank.com", &st.never_analyse, normalize_site, true, true, move |next| m.edit(|d| d.settings.never_analyse = next)));
    }
    sec.append(&caption("Jev never sees these sites, Kairos Read reads them with the built-in voice and makes no scenes from them. Jev is only told a browser\u{2019}s name, never its page title."));
    b.append(&outer);

    // Data
    let (outer, sec) = section("Data");
    let row = hbox(8);
    row.append(&button("Export everything as JSON\u{2026}", {
        let m = model.clone();
        move || export(&m)
    }));
    let import_btn = button("Import\u{2026}", {
        let m = model.clone();
        move || import_file(&m)
    });
    import_btn.set_sensitive(s.can_import());
    import_btn.set_tooltip_text(Some(if s.can_import() { "Replace everything with an exported file" } else { "Stop the timer to import." }));
    row.append(&import_btn);
    sec.append(&row);
    sec.append(&caption("Import replaces your groups, tasks, rules, history and settings with an exported file, from Kairos on Linux or on a Mac. Kairos saves a copy of what you have now first, beside its data file."));
    b.append(&outer);
    drop(s);
    scrolled(&b).upcast()
}

fn setter(model: &Rc<Model>, f: fn(&mut AppData, bool)) -> impl Fn(bool) + 'static {
    let m = model.clone();
    move |on| {
        let m = m.clone();
        glib::idle_add_local_once(move || m.edit(|d| f(d, on)));
    }
}

fn labeled_spin(title: &str, min: f64, max: f64, step: f64, value: f64, f: impl Fn(f64) + 'static) -> gtk::Box {
    let b = hbox(4);
    b.append(&caption(title));
    b.append(&spin(min, max, step, value, f));
    b
}

fn hotkey_picker(model: &Rc<Model>, title: &str, choices: &'static [hotkeys::HotkeyChoice], current: &str, set: fn(&mut AppData, String)) -> gtk::Box {
    let mut labels: Vec<String> = choices.iter().map(|c| c.label.to_string()).collect();
    labels.push("Off".into());
    let cur = if current == hotkeys::OFF { choices.len() } else { choices.iter().position(|c| c.id == current).unwrap_or(0) };
    let m = model.clone();
    labeled(title, &dropdown(&labels, cur, move |i| {
        let v = choices.get(i).map(|c| c.id.to_string()).unwrap_or_else(|| hotkeys::OFF.to_string());
        let m = m.clone();
        glib::idle_add_local_once(move || m.edit(|d| set(d, v)));
    }))
}

fn key_row(model: &Rc<Model>, title: &str, has: bool, account: &'static str) -> gtk::Box {
    let row = hbox(8);
    let e = gtk::PasswordEntry::new();
    e.set_show_peek_icon(true);
    e.set_hexpand(true);
    e.set_property("placeholder-text", if has { format!("{title}: saved \u{2014} type to replace") } else { format!("{title}: paste key") });
    e.update_property(&[gtk::accessible::Property::Label(title)]);
    let save = gtk::Button::with_label("Save");
    save.set_sensitive(false);
    {
        let s = save.clone();
        e.connect_changed(move |e| s.set_sensitive(!e.text().is_empty()));
    }
    {
        let (m, e2) = (model.clone(), e.clone());
        save.connect_clicked(move |_| {
            let v = e2.text().to_string();
            e2.set_text("");
            let m = m.clone();
            glib::idle_add_local_once(move || m.set_key(&v, account));
        });
    }
    row.append(&e);
    row.append(&save);
    if has {
        row.append(&button("Remove", act(model, move |m| m.set_key("", account))));
    }
    row
}

fn export(model: &Rc<Model>) {
    let d = gtk::FileDialog::builder().title("Export Kairos data").initial_name("kairos-export.json").modal(true).build();
    let m = model.clone();
    d.save(main_window(model).as_ref(), gio::Cancellable::NONE, move |r| {
        if let Ok(file) = r {
            if let Some(path) = file.path() {
                if let Err(e) = std::fs::write(&path, m.export_json()) {
                    alert(main_window(&m).as_ref(), "Export failed", &e.to_string());
                }
            }
        }
    });
}

fn import_file(model: &Rc<Model>) {
    let filter = gtk::FileFilter::new();
    filter.add_pattern("*.json");
    filter.set_name(Some("Kairos export (JSON)"));
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let d = gtk::FileDialog::builder().title("Import Kairos data").filters(&filters).modal(true).build();
    let m = model.clone();
    d.open(main_window(model).as_ref(), gio::Cancellable::NONE, move |r| {
        let Ok(file) = r else { return };
        let Some(path) = file.path() else { return };
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let parent = main_window(&m);
        let parsed = std::fs::read(&path).ok().and_then(|raw| AppData::from_export(&raw).ok());
        let Some(data) = parsed else {
            alert(parent.as_ref(), &format!("\u{201C}{name}\u{201D} is not a Kairos export"), "Choose a file made with \u{201C}Export everything as JSON\u{201D}. Nothing was changed.");
            return;
        };
        let (g, t) = (data.groups.len(), data.tasks.len());
        let detail = format!(
            "{g} group{} and {t} task{} from \u{201C}{name}\u{201D} take the place of yours. Kairos saves a copy of your current data first.",
            if g == 1 { "" } else { "s" },
            if t == 1 { "" } else { "s" }
        );
        let m2 = m.clone();
        let data = RefCell::new(Some(data));
        ask(parent.as_ref(), "Replace everything in Kairos with this file?", &detail, &["Replace", "Cancel"], 1, move |choice| {
            if choice != Some(0) {
                return;
            }
            let Some(data) = data.borrow_mut().take() else { return };
            let parent = main_window(&m2);
            match m2.import_data(data) {
                Ok(Some(backup)) => {
                    let folder = backup.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                    let detail = format!(
                        "Your previous data is in \u{201C}{}\u{201D}, in {}.",
                        backup.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                        folder.display()
                    );
                    ask(parent.as_ref(), "Imported", &detail, &["OK", "Show folder"], 0, move |c| {
                        if c == Some(1) {
                            system::show_folder(&folder);
                        }
                    });
                }
                Ok(None) => alert(parent.as_ref(), "Imported", "There was no saved data to back up."),
                Err(e) => alert(parent.as_ref(), "Import failed", &format!("Nothing was changed. {e}")),
            }
        });
    });
}

// MARK: First launch

pub fn onboarding(model: &Rc<Model>, parent: &gtk::ApplicationWindow) -> gtk::Window {
    let win = gtk::Window::builder().title("Welcome to Kairos").modal(true).resizable(false).default_width(520).transient_for(parent).build();
    let b = vbox(16);
    b.set_margin_top(26);
    b.set_margin_bottom(26);
    b.set_margin_start(26);
    b.set_margin_end(26);
    b.append(&styled("Welcome to Kairos", "kairos-title"));
    b.append(&styled("Kairos shows only the apps for what you are doing, blocks the rest while you focus, and times your work.", "kairos-dim"));
    let browsers = bridge_installer::installed_browsers();
    let key = hotkeys::choice(&model.s().data.settings.quick_add_hotkey, hotkeys::QUICK_ADD).map(|c| c.label).unwrap_or("the quick add shortcut");
    let set_up = if browsers.is_empty() {
        "Today lists what is left to set up: the Kairos Shield extension (no Chromium browser was found yet), a key for Jev, which checks whether a page or app fits your current task, and 3D scenes on your Claude subscription.".to_string()
    } else {
        format!("Today lists what is left to set up: loading the Kairos Shield and Kairos Read extensions, a key for Jev, which checks whether a page or app fits your current task, and 3D scenes on your Claude subscription. The browser link is installed for {}.", browsers.join(", "))
    };
    for (n, title, text) in [
        ("1", "Groups", "You start with Work, Study and Personal, each with its own apps, blocked sites and timer. Switch group from Kairos Bar or the tray icon, and add one with the + on Kairos Bar.".to_string()),
        ("2", "Tasks", format!("Press {key} anywhere to add a task, like \u{201C}Draft report tomorrow 3pm\u{201D}. Pick one on Today and start a focus session.")),
        ("3", "Set up", set_up),
    ] {
        let row = hbox(12);
        let num = gtk::Label::new(Some(n));
        num.add_css_class("kairos-step");
        num.set_valign(gtk::Align::Start);
        row.append(&num);
        let t = vbox(2);
        t.append(&heading(title));
        t.append(&styled(&text, "kairos-dim"));
        row.append(&t);
        b.append(&row);
    }
    let foot = hbox(8);
    foot.append(&spacer());
    let (m, w) = (model.clone(), win.clone());
    let go = suggested(button("Go to Today", move || {
        m.set(change::NAV, |s| s.page = Page::Today);
        m.edit(|d| d.onboarded = true);
        w.close();
    }));
    foot.append(&go);
    b.append(&foot);
    win.set_child(Some(&b));
    win.set_default_widget(Some(&go));
    win
}

#[allow(dead_code)]
fn unused(_: impl Fn()) {}
#[allow(dead_code)]
fn use_later(m: &Rc<Model>) -> impl Fn() {
    later(m, |_| {})
}

fn clicked(f: impl Fn() + 'static) -> impl Fn(&gtk::Button) + 'static {
    move |_| f()
}
