//! Group colours and icons, and icon images for Kairos Bar. Groups store the Mac app's SF Symbol
//! names, so exports move between the two; here each shows as a freedesktop symbolic icon.

use gtk::gdk_pixbuf::Pixbuf;
use gtk::gio;
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

pub const COLORS: &[(&str, &str)] = &[
    ("#4F7CFF", "Blue"),
    ("#2FA36B", "Green"),
    ("#E0703A", "Orange"),
    ("#B45BD6", "Purple"),
    ("#D6455B", "Red"),
    ("#2AA7B8", "Teal"),
    ("#C9A227", "Yellow"),
    ("#6B7280", "Grey"),
];

/// (SF Symbol stored in the data, freedesktop icon shown, name).
pub const GROUP_ICONS: &[(&str, &str, &str)] = &[
    ("briefcase.fill", "emblem-documents-symbolic", "Briefcase"),
    ("book.fill", "accessories-dictionary-symbolic", "Book"),
    ("house.fill", "user-home-symbolic", "House"),
    ("paintbrush.fill", "applications-graphics-symbolic", "Paintbrush"),
    ("hammer.fill", "applications-engineering-symbolic", "Hammer"),
    ("gamecontroller.fill", "applications-games-symbolic", "Game controller"),
    ("music.note", "audio-x-generic-symbolic", "Music"),
    ("graduationcap.fill", "applications-science-symbolic", "Graduation cap"),
    ("chevron.left.forwardslash.chevron.right", "text-x-script-symbolic", "Code"),
    ("pencil", "document-edit-symbolic", "Pencil"),
    ("heart.fill", "emblem-favorite-symbolic", "Heart"),
    ("square.grid.2x2", "view-grid-symbolic", "Grid"),
];

pub fn group_icon(symbol: &str) -> &'static str {
    GROUP_ICONS.iter().find(|i| i.0 == symbol).map(|i| i.1).unwrap_or("view-grid-symbolic")
}

pub fn icon_label(symbol: &str) -> String {
    GROUP_ICONS.iter().find(|i| i.0 == symbol).map(|i| i.2.to_string()).unwrap_or_else(|| symbol.to_string())
}

pub fn rgb(hex: &str) -> (f64, f64, f64) {
    let s = hex.trim().trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).unwrap_or(0x4F7CFF);
    (((v >> 16) & 0xff) as f64 / 255.0, ((v >> 8) & 0xff) as f64 / 255.0, (v & 0xff) as f64 / 255.0)
}

pub fn rgb8(hex: &str) -> (u8, u8, u8) {
    let (r, g, b) = rgb(hex);
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

thread_local! {
    static CACHE: RefCell<HashMap<(String, i32), Option<Pixbuf>>> = RefCell::new(HashMap::new());
}

fn load(icon: &gio::Icon, px: i32) -> Option<Pixbuf> {
    let display = gtk::gdk::Display::default()?;
    let theme = gtk::IconTheme::for_display(&display);
    let paintable = theme.lookup_by_gicon(icon, px, 1, gtk::TextDirection::None, gtk::IconLookupFlags::empty());
    let path = paintable.file()?.path()?;
    Pixbuf::from_file_at_scale(path, px, px, true).ok()
}

/// An image for an app or folder icon, `px` pixels square, cached.
pub fn pixbuf(key: &str, icon: Option<gio::Icon>, px: i32) -> Option<Pixbuf> {
    let k = (key.to_string(), px);
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&k).cloned()) {
        return hit;
    }
    let icon = icon.unwrap_or_else(|| gio::ThemedIcon::new("application-x-executable").upcast());
    let pb = load(&icon, px).or_else(|| load(&gio::ThemedIcon::new("application-x-executable").upcast(), px));
    CACHE.with(|c| c.borrow_mut().insert(k, pb.clone()));
    pb
}

pub fn named(name: &str, px: i32) -> Option<Pixbuf> {
    pixbuf(&format!("icon:{name}"), Some(gio::ThemedIcon::new(name).upcast()), px)
}

pub fn forget(key_prefix: &str) {
    CACHE.with(|c| c.borrow_mut().retain(|k, _| !k.0.starts_with(key_prefix)));
}
