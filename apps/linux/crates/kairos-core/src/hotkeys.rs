//! Global shortcuts the owner can pick in Settings. Keys are X11 keysyms and modifier masks,
//! which XGrabKey takes. Ids are shared with the Mac app (Option is Alt here, Command is Super),
//! so a Mac export keeps its shortcuts where the keys exist.

use crate::models::Settings;

pub const SHIFT: u16 = 1 << 0;
pub const CONTROL: u16 = 1 << 2;
pub const ALT: u16 = 1 << 3; // Mod1
pub const SUPER: u16 = 1 << 6; // Mod4

pub const XK_SPACE: u32 = 0x0020;
pub const XK_N: u32 = 0x006e;
pub const XK_T: u32 = 0x0074;
pub const XK_P: u32 = 0x0070;
pub const XK_1: u32 = 0x0031;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotkeyChoice {
    pub id: &'static str,
    pub label: &'static str,
    pub keysym: u32,
    pub modifiers: u16,
}

pub const OFF: &str = "off";

/// Add a task from anywhere. Alt+Space opens Cinnamon's window menu, so it comes last.
pub const QUICK_ADD: &[HotkeyChoice] = &[
    HotkeyChoice { id: "control-option-space", label: "Ctrl+Alt+Space", keysym: XK_SPACE, modifiers: CONTROL | ALT },
    HotkeyChoice { id: "control-option-n", label: "Ctrl+Alt+N", keysym: XK_N, modifiers: CONTROL | ALT },
    HotkeyChoice { id: "super-space", label: "Super+Space", keysym: XK_SPACE, modifiers: SUPER },
    HotkeyChoice { id: "option-space", label: "Alt+Space", keysym: XK_SPACE, modifiers: ALT },
];

/// Start, pause or resume the timer from anywhere. Ctrl+Alt+T opens a terminal on Mint.
pub const TIMER: &[HotkeyChoice] = &[
    HotkeyChoice { id: "control-option-p", label: "Ctrl+Alt+P", keysym: XK_P, modifiers: CONTROL | ALT },
    HotkeyChoice { id: "option-shift-space", label: "Alt+Shift+Space", keysym: XK_SPACE, modifiers: ALT | SHIFT },
    HotkeyChoice { id: "control-option-t", label: "Ctrl+Alt+T", keysym: XK_T, modifiers: CONTROL | ALT },
];

/// The choice stored as `id`, or None when it is off. An unknown id falls back to the first.
pub fn choice<'a>(id: &str, choices: &'a [HotkeyChoice]) -> Option<&'a HotkeyChoice> {
    if id == OFF {
        return None;
    }
    choices.iter().find(|c| c.id == id).or_else(|| choices.first())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    QuickAdd,
    Timer,
    Group(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub action: Action,
    pub label: String,
    pub keysym: u32,
    pub modifiers: u16,
}

/// The global shortcuts Kairos grabs, from Settings. Grabbed again whenever it changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotkeyPlan {
    pub entries: Vec<Entry>,
}

impl HotkeyPlan {
    pub fn new(s: &Settings) -> Self {
        let mut entries = vec![];
        if let Some(c) = choice(&s.quick_add_hotkey, QUICK_ADD) {
            entries.push(Entry { action: Action::QuickAdd, label: c.label.into(), keysym: c.keysym, modifiers: c.modifiers });
        }
        if let Some(c) = choice(&s.timer_hotkey, TIMER) {
            entries.push(Entry { action: Action::Timer, label: c.label.into(), keysym: c.keysym, modifiers: c.modifiers });
        }
        if s.group_hotkeys {
            for i in 0..9 {
                entries.push(Entry { action: Action::Group(i), label: format!("Ctrl+{}", i + 1), keysym: XK_1 + i as u32, modifiers: CONTROL });
            }
        }
        HotkeyPlan { entries }
    }

    /// What Settings says when some shortcuts could not be grabbed.
    pub fn failure_text(labels: &[String]) -> Option<String> {
        if labels.is_empty() {
            return None;
        }
        let list = match labels.len() {
            1 => labels[0].clone(),
            n => format!("{} and {}", labels[..n - 1].join(", "), labels[n - 1]),
        };
        Some(if labels.len() == 1 {
            format!("{list} is already taken by another app or by Cinnamon, so it does nothing in Kairos. Pick another shortcut or free it in System Settings > Keyboard.")
        } else {
            format!("{list} are already taken by other apps or by Cinnamon, so they do nothing in Kairos. Pick other shortcuts or free them in System Settings > Keyboard.")
        })
    }
}
