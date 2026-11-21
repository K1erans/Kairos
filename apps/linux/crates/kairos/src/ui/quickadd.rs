//! The quick add box: a line in the middle of the screen, opened by the global shortcut. It reads
//! dates, times, @group, ~estimate and !priority from the text and shows what it understood.

use super::widgets::*;
use crate::model::Model;
use gtk::glib;
use gtk::prelude::*;
use kairos_core::{duetext, quickadd, time};
use std::rc::Rc;

pub fn show(app: &gtk::Application, model: &Rc<Model>) {
    if let Some(w) = app.windows().into_iter().find(|w| w.widget_name() == "kairos-quickadd") {
        w.present();
        bring_to_front(model.x.as_ref(), &w);
        return;
    }
    let win = gtk::Window::builder().application(app).decorated(false).resizable(false).default_width(620).title("Quick add").build();
    win.set_widget_name("kairos-quickadd");
    win.add_css_class("kairos-transparent");
    let b = vbox(8);
    b.add_css_class("kairos-quickadd");
    b.set_margin_top(10);
    b.set_margin_bottom(10);
    b.set_margin_start(10);
    b.set_margin_end(10);
    let row = hbox(10);
    let icon = gtk::Image::from_icon_name("list-add-symbolic");
    icon.set_pixel_size(22);
    row.append(&icon);
    let entry = gtk::Entry::new();
    entry.set_hexpand(true);
    entry.set_placeholder_text(Some("Draft report tomorrow 3pm @Work ~45m !high"));
    row.append(&entry);
    b.append(&row);
    let preview = styled("", "kairos-dim");
    preview.set_visible(false);
    b.append(&preview);
    win.set_child(Some(&b));
    {
        let (m, preview) = (model.clone(), preview.clone());
        entry.connect_changed(move |e| {
            let text = e.text().to_string();
            if text.trim().is_empty() {
                preview.set_visible(false);
                return;
            }
            let names: Vec<String> = m.s().data.groups.iter().map(|g| g.name.clone()).collect();
            let now = time::now();
            let p = quickadd::parse(&text, &names, now);
            let mut parts = vec![if p.title.is_empty() { "Untitled".to_string() } else { format!("\u{201C}{}\u{201D}", p.title) }];
            if let Some(d) = p.due {
                parts.push(format!("due {}", duetext::short_local(d, now)));
            }
            if let Some(g) = p.group_name {
                parts.push(format!("in {g}"));
            }
            if let Some(e) = p.estimate_min {
                parts.push(format!("{e} min"));
            }
            if p.priority > 0 {
                parts.push(["", "low", "medium", "high"][p.priority.clamp(0, 3) as usize].to_string() + " priority");
            }
            preview.set_text(&parts.join(" \u{00B7} "));
            preview.set_visible(true);
        });
    }
    {
        let (m, w) = (model.clone(), win.clone());
        entry.connect_activate(move |e| {
            let text = e.text().to_string();
            if !text.trim().is_empty() {
                m.add_task(&text);
            }
            w.close();
        });
    }
    let keys = gtk::EventControllerKey::new();
    {
        let w = win.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                w.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    win.add_controller(keys);
    // Closes when it loses focus, like Spotlight.
    {
        let w = win.clone();
        win.connect_is_active_notify(move |win| {
            if !win.is_active() && win.is_visible() {
                let w = w.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
                    if !w.is_active() {
                        w.close();
                    }
                });
            }
        });
    }
    win.present();
    // Opened from a global shortcut, so it needs activating to get the keyboard.
    bring_to_front(model.x.as_ref(), &win);
    entry.grab_focus();
}
