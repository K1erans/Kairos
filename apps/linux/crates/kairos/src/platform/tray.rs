//! The status icon in Cinnamon's panel (StatusNotifierItem), standing in for the macOS menu bar
//! item. Its menu is built from a snapshot the app pushes on every change; choices go back to
//! the main loop as commands.

use kairos_core::Mode;

#[derive(Clone, Debug, PartialEq)]
pub enum TrayCommand {
    MarkDone,
    StartFocus,
    NotNow,
    ToggleTimer,
    Skip,
    AddFive,
    Stop,
    ReadToggle,
    SwitchGroup(usize),
    NewGroup,
    SetMode(Mode),
    QuickAdd,
    Open,
    ToggleBar,
    Quit,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct TraySnapshot {
    pub title: String,
    pub color: (u8, u8, u8),
    pub progress: Option<f64>,
    pub blocking_off: bool,
    pub current_task: Option<String>,
    pub idle: bool,
    pub awaiting: bool,
    pub phase_line: String,
    pub paused: bool,
    pub strict_locked: bool,
    pub reading: Option<bool>,
    pub groups: Vec<String>,
    pub active_group: usize,
    pub mode: usize,
    pub mode_locked: bool,
    pub quick_add_key: Option<String>,
    pub timer_key: Option<String>,
    pub bar_visible: bool,
    pub quit_note: Option<String>,
}

pub struct KairosTray {
    pub snap: TraySnapshot,
    pub send: async_channel::Sender<TrayCommand>,
}

fn with_key(label: &str, key: &Option<String>) -> String {
    match key {
        Some(k) => format!("{label}\t{k}"),
        None => label.to_string(),
    }
}

impl KairosTray {
    fn cmd(c: TrayCommand) -> Box<dyn Fn(&mut Self) + Send> {
        Box::new(move |t: &mut Self| {
            let _ = t.send.send_blocking(c.clone());
        })
    }

    fn item(label: impl Into<String>, enabled: bool, c: Option<TrayCommand>) -> ksni::MenuItem<Self> {
        ksni::menu::StandardItem {
            label: label.into(),
            enabled,
            activate: match c {
                Some(c) => Self::cmd(c),
                None => Box::new(|_| {}),
            },
            ..Default::default()
        }
        .into()
    }
}

impl ksni::Tray for KairosTray {
    fn id(&self) -> String {
        "kairos".into()
    }
    fn title(&self) -> String {
        self.snap.title.clone()
    }
    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: self.snap.title.clone(), description: self.snap.phase_line.clone(), ..Default::default() }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.send.send_blocking(TrayCommand::Open);
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        [22, 32, 48].iter().map(|s| icon(*s, self.snap.color, self.snap.progress)).collect()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let s = &self.snap;
        let mut m: Vec<ksni::MenuItem<Self>> = vec![];
        if s.blocking_off {
            m.push(Self::item("\u{26A0} Site blocking is off: Kairos Shield isn\u{2019}t connected", false, None));
            m.push(MenuItem::Separator);
        }
        match &s.current_task {
            Some(t) => {
                m.push(Self::item(t.clone(), false, None));
                m.push(Self::item("Mark done", true, Some(TrayCommand::MarkDone)));
            }
            None => m.push(Self::item("No current task", false, None)),
        }
        m.push(MenuItem::Separator);
        if s.idle {
            if s.awaiting {
                m.push(Self::item("Break over", false, None));
            }
            m.push(Self::item(with_key("Start focus", &s.timer_key), true, Some(TrayCommand::StartFocus)));
            if s.awaiting {
                m.push(Self::item("Not now", true, Some(TrayCommand::NotNow)));
            }
        } else {
            m.push(Self::item(s.phase_line.clone(), false, None));
            m.push(Self::item(with_key(if s.paused { "Resume" } else { "Pause" }, &s.timer_key), true, Some(TrayCommand::ToggleTimer)));
            m.push(Self::item("Skip", !s.strict_locked, Some(TrayCommand::Skip)));
            m.push(Self::item("+5 min", true, Some(TrayCommand::AddFive)));
            m.push(Self::item("Stop", !s.strict_locked, Some(TrayCommand::Stop)));
        }
        if let Some(playing) = s.reading {
            m.push(MenuItem::Separator);
            m.push(Self::item(if playing { "Pause reading" } else { "Resume reading" }, true, Some(TrayCommand::ReadToggle)));
        }
        m.push(MenuItem::Separator);
        m.push(
            SubMenu {
                label: "Group".into(),
                submenu: vec![RadioGroup {
                    selected: s.active_group,
                    select: Box::new(|t: &mut Self, i| {
                        let _ = t.send.send_blocking(TrayCommand::SwitchGroup(i));
                    }),
                    options: s.groups.iter().map(|g| RadioItem { label: g.clone(), ..Default::default() }).collect(),
                }
                .into()],
                ..Default::default()
            }
            .into(),
        );
        m.push(Self::item("New Group\u{2026}", true, Some(TrayCommand::NewGroup)));
        m.push(
            SubMenu {
                label: "Mode".into(),
                enabled: !s.mode_locked,
                submenu: vec![RadioGroup {
                    selected: s.mode,
                    select: Box::new(|t: &mut Self, i| {
                        let _ = t.send.send_blocking(TrayCommand::SetMode(Mode::ALL[i]));
                    }),
                    options: Mode::ALL.iter().map(|md| RadioItem { label: md.label().into(), enabled: !s.mode_locked, ..Default::default() }).collect(),
                }
                .into()],
                ..Default::default()
            }
            .into(),
        );
        m.push(MenuItem::Separator);
        m.push(Self::item(with_key("Quick add\u{2026}", &s.quick_add_key), true, Some(TrayCommand::QuickAdd)));
        m.push(Self::item("Open Kairos", true, Some(TrayCommand::Open)));
        m.push(
            CheckmarkItem { label: "Show Kairos Bar".into(), checked: s.bar_visible, activate: Self::cmd(TrayCommand::ToggleBar), ..Default::default() }
                .into(),
        );
        m.push(MenuItem::Separator);
        if let Some(note) = &s.quit_note {
            m.push(Self::item(note.clone(), false, None));
        }
        m.push(Self::item("Quit Kairos", s.quit_note.is_none(), Some(TrayCommand::Quit)));
        m
    }
}

/// A dot in the active group's colour, with a ring that fills as the phase runs.
fn icon(size: i32, (r, g, b): (u8, u8, u8), progress: Option<f64>) -> ksni::Icon {
    let n = size as usize;
    let mut data = vec![0u8; n * n * 4];
    let c = size as f64 / 2.0;
    let outer = c - 1.0;
    let inner = outer * if progress.is_some() { 0.52 } else { 0.62 };
    let ring_in = outer * 0.74;
    for y in 0..n {
        for x in 0..n {
            let dx = x as f64 + 0.5 - c;
            let dy = y as f64 + 0.5 - c;
            let d = (dx * dx + dy * dy).sqrt();
            let mut a = 0.0f64;
            let mut col = (r, g, b);
            if d <= inner {
                a = (inner - d + 0.5).clamp(0.0, 1.0);
            } else if let Some(p) = progress {
                if d >= ring_in && d <= outer {
                    let angle = (dx.atan2(-dy) / std::f64::consts::TAU).rem_euclid(1.0);
                    let edge = (outer - d + 0.5).clamp(0.0, 1.0).min((d - ring_in + 0.5).clamp(0.0, 1.0));
                    if angle <= p {
                        a = edge;
                    } else {
                        a = edge * 0.35;
                        col = (200, 200, 200);
                    }
                }
            }
            let i = (y * n + x) * 4;
            data[i] = (a * 255.0) as u8;
            data[i + 1] = col.0;
            data[i + 2] = col.1;
            data[i + 3] = col.2;
        }
    }
    ksni::Icon { width: size, height: size, data }
}
