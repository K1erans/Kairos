use crate::models::{Mode, TempoPreset};
use crate::time::{self, add, since, Date};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TempoPhase {
    #[default]
    Idle,
    Focus,
    ShortBreak,
    LongBreak,
}

impl TempoPhase {
    pub fn is_break(self) -> bool {
        matches!(self, TempoPhase::ShortBreak | TempoPhase::LongBreak)
    }
    pub fn label(self) -> &'static str {
        match self {
            TempoPhase::Idle => "Idle",
            TempoPhase::Focus => "Focus",
            TempoPhase::ShortBreak => "Short break",
            TempoPhase::LongBreak => "Long break",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TempoEvent {
    /// A phase ran to its end (or was skipped), with active `duration` excluding pauses.
    Finished { phase: TempoPhase, start: Date, end: Date, duration: f64, interruptions: i32, completed: bool },
    Started(TempoPhase),
    /// The timer went idle: stopped by hand, or the machine slept through the next phase.
    Stopped,
    /// A break ran out and the timer went idle to wait for the owner to start the next focus phase.
    AwaitingFocus,
}

/// A strict focus phase that was running when Kairos quit, to pick up at launch.
#[derive(Clone, Debug, PartialEq)]
pub struct StrictResume {
    pub started_at: Date,
    pub ends_at: Date,
    pub paused_duration: f64,
}

impl StrictResume {
    pub fn new(started_at: Date, ends_at: Date, paused_duration: f64) -> Self {
        StrictResume { started_at, ends_at, paused_duration: paused_duration.max(0.0) }
    }
}

/// What Kairos asks before a strict focus phase starts, naming when it ends.
#[derive(Clone, Debug, PartialEq)]
pub struct StrictConfirmation {
    pub title: String,
    pub message: String,
}

impl StrictConfirmation {
    pub fn new(minutes: i32, ends_at: Date) -> Self {
        let t = time::short_time(ends_at);
        StrictConfirmation {
            title: format!("Start strict focus until {t}?"),
            message: format!(
                "For the next {minutes} minutes, until {t}, this session cannot be stopped, skipped or quit, \
                 and there are no \u{201C}Allow\u{201D} overrides. Blocked apps are hidden and blocked sites stay blocked, \
                 even if Kairos is closed or the computer restarts."
            ),
        }
    }
}

/// Pomodoro state machine. Time is stored as `ends_at`, never as a ticking counter, so sleep
/// and suspend cannot make it drift.
#[derive(Clone, Debug, PartialEq)]
pub struct TempoEngine {
    phase: TempoPhase,
    started_at: Option<Date>,
    ends_at: Option<Date>,
    /// Seconds left when paused; None while running.
    paused_remaining: Option<f64>,
    paused_at: Option<Date>,
    paused_duration: f64,
    /// Full length of the current phase, including any "+5 min".
    total: f64,
    focus_count: i32,
    interruptions: i32,
    preset: TempoPreset,
    pending_preset: Option<TempoPreset>,
    /// Strict chosen by hand when the timer started. Holds until the timer stops.
    holds_strict: bool,
    awaiting_focus: bool,
}

impl TempoEngine {
    pub fn new(preset: TempoPreset) -> Self {
        TempoEngine {
            phase: TempoPhase::Idle,
            started_at: None,
            ends_at: None,
            paused_remaining: None,
            paused_at: None,
            paused_duration: 0.0,
            total: 0.0,
            focus_count: 0,
            interruptions: 0,
            preset,
            pending_preset: None,
            holds_strict: false,
            awaiting_focus: false,
        }
    }

    pub fn phase(&self) -> TempoPhase {
        self.phase
    }
    pub fn started_at(&self) -> Option<Date> {
        self.started_at
    }
    pub fn ends_at(&self) -> Option<Date> {
        self.ends_at
    }
    pub fn total(&self) -> f64 {
        self.total
    }
    pub fn focus_count(&self) -> i32 {
        self.focus_count
    }
    pub fn interruptions(&self) -> i32 {
        self.interruptions
    }
    pub fn preset(&self) -> &TempoPreset {
        &self.preset
    }
    pub fn awaiting_focus(&self) -> bool {
        self.awaiting_focus
    }
    pub fn is_running(&self) -> bool {
        self.phase != TempoPhase::Idle && self.paused_remaining.is_none()
    }
    pub fn is_paused(&self) -> bool {
        self.paused_remaining.is_some()
    }

    /// The mode this phase puts Kairos in.
    pub fn mode(&self) -> Mode {
        match self.phase {
            TempoPhase::Idle => Mode::Off,
            TempoPhase::Focus => {
                if self.holds_strict || self.preset.strict {
                    Mode::Strict
                } else {
                    Mode::Focus
                }
            }
            _ => Mode::OnBreak,
        }
    }

    /// Applies at once when idle, otherwise from the next phase.
    pub fn use_preset(&mut self, preset: TempoPreset) {
        if self.phase == TempoPhase::Idle {
            self.preset = preset;
            self.pending_preset = None;
        } else {
            self.pending_preset = Some(preset);
        }
    }

    pub fn remaining(&self, now: Date) -> f64 {
        if let Some(r) = self.paused_remaining {
            return r;
        }
        match self.ends_at {
            Some(e) => since(e, now).max(0.0),
            None => 0.0,
        }
    }

    pub fn duration_of(&self, phase: TempoPhase) -> f64 {
        60.0 * match phase {
            TempoPhase::Idle => 0,
            TempoPhase::Focus => self.preset.focus_min,
            TempoPhase::ShortBreak => self.preset.short_break_min,
            TempoPhase::LongBreak => self.preset.long_break_min,
        } as f64
    }

    pub fn progress(&self, now: Date) -> f64 {
        if self.phase == TempoPhase::Idle || self.total <= 0.0 {
            return 0.0;
        }
        (1.0 - self.remaining(now) / self.total).clamp(0.0, 1.0)
    }

    /// Starts a new timer. `strict` holds strict mode for every focus phase until it stops.
    pub fn start(&mut self, new_phase: TempoPhase, strict: bool, now: Date) -> Vec<TempoEvent> {
        if new_phase == TempoPhase::Idle {
            return self.stop(now);
        }
        let mut events = self.close_current(now, false);
        self.take_pending_preset();
        if !(self.awaiting_focus && new_phase == TempoPhase::Focus) {
            self.focus_count = 0;
        }
        self.awaiting_focus = false;
        self.holds_strict = strict;
        self.begin(new_phase, now);
        events.push(TempoEvent::Started(new_phase));
        events
    }

    /// Picks up a strict focus phase saved before a quit.
    pub fn resume_strict(&mut self, saved: &StrictResume) {
        self.take_pending_preset();
        self.phase = TempoPhase::Focus;
        self.started_at = Some(saved.started_at);
        self.ends_at = Some(saved.ends_at);
        self.paused_duration = saved.paused_duration.max(0.0);
        self.total = (since(saved.ends_at, saved.started_at) - self.paused_duration).max(0.0);
        self.paused_remaining = None;
        self.paused_at = None;
        self.interruptions = 0;
        self.focus_count = 0;
        self.holds_strict = true;
    }

    /// "Not now" after a break ran out: stop waiting.
    pub fn end_wait(&mut self) {
        if !self.awaiting_focus {
            return;
        }
        self.awaiting_focus = false;
        self.focus_count = 0;
    }

    /// Whether the next focus phase of a running timer is strict.
    pub fn next_focus_is_strict(&self) -> bool {
        self.holds_strict || self.pending_preset.as_ref().unwrap_or(&self.preset).strict
    }

    pub fn pause(&mut self, now: Date) {
        if !self.is_running() || self.remaining(now) <= 0.0 {
            return;
        }
        self.paused_remaining = Some(self.remaining(now));
        self.paused_at = Some(now);
        self.interruptions += 1;
    }

    pub fn resume(&mut self, now: Date) {
        let Some(left) = self.paused_remaining else { return };
        if let Some(p) = self.paused_at {
            self.paused_duration += since(now, p).max(0.0);
        }
        self.paused_remaining = None;
        self.paused_at = None;
        self.ends_at = Some(add(now, left));
    }

    pub fn extend(&mut self, minutes: i32, now: Date) {
        if self.phase == TempoPhase::Idle {
            return;
        }
        let extra = (minutes * 60) as f64;
        self.total += extra;
        if let Some(left) = self.paused_remaining {
            self.paused_remaining = Some(left + extra);
        } else if let Some(e) = self.ends_at {
            self.ends_at = Some(add(e.max(now), extra));
        }
    }

    pub fn skip(&mut self, now: Date) -> Vec<TempoEvent> {
        if self.phase == TempoPhase::Idle {
            return vec![];
        }
        if self.phase == TempoPhase::Focus {
            self.interruptions += 1;
        }
        self.advance(now, false, now, false)
    }

    pub fn stop(&mut self, now: Date) -> Vec<TempoEvent> {
        let mut events = self.close_current(now, false);
        self.reset();
        events.push(TempoEvent::Stopped);
        events
    }

    /// Call regularly (and on wake). Ends a phase that has run out at its own end time.
    pub fn tick(&mut self, now: Date, starts_focus_after_break: bool) -> Vec<TempoEvent> {
        let Some(end) = self.ends_at else { return vec![] };
        if !self.is_running() || now < end {
            return vec![];
        }
        self.advance(end, true, now, !starts_focus_after_break)
    }

    /// The running phase as a session cut short at `now`, for saving before a quit.
    pub fn current(&self, now: Date) -> Option<TempoEvent> {
        self.close_current(now, false).into_iter().next()
    }

    fn advance(&mut self, end: Date, completed: bool, now: Date, waits_after_break: bool) -> Vec<TempoEvent> {
        let finished = self.phase;
        let mut events = self.close_current(end, completed);
        self.take_pending_preset();
        let next = if finished == TempoPhase::Focus {
            if completed {
                self.focus_count += 1;
            }
            let long = completed && self.preset.cycles_before_long > 0 && self.focus_count % self.preset.cycles_before_long == 0;
            if long {
                TempoPhase::LongBreak
            } else {
                TempoPhase::ShortBreak
            }
        } else {
            TempoPhase::Focus
        };
        if now > end && now >= add(end, self.duration_of(next)) {
            self.reset();
            events.push(TempoEvent::Stopped);
            return events;
        }
        if waits_after_break && finished.is_break() {
            let count = self.focus_count;
            self.reset();
            self.focus_count = count;
            self.awaiting_focus = true;
            events.push(TempoEvent::AwaitingFocus);
            return events;
        }
        self.begin(next, end);
        events.push(TempoEvent::Started(next));
        events
    }

    fn take_pending_preset(&mut self) {
        if let Some(p) = self.pending_preset.take() {
            self.preset = p;
        }
    }

    fn reset(&mut self) {
        self.take_pending_preset();
        self.phase = TempoPhase::Idle;
        self.started_at = None;
        self.ends_at = None;
        self.paused_remaining = None;
        self.paused_at = None;
        self.paused_duration = 0.0;
        self.interruptions = 0;
        self.total = 0.0;
        self.focus_count = 0;
        self.holds_strict = false;
        self.awaiting_focus = false;
    }

    fn close_current(&self, now: Date, completed: bool) -> Vec<TempoEvent> {
        let (TempoPhase::Focus | TempoPhase::ShortBreak | TempoPhase::LongBreak) = self.phase else { return vec![] };
        let Some(start) = self.started_at else { return vec![] };
        let end = start.max(self.paused_at.unwrap_or(now));
        let active = (since(end, start) - self.paused_duration).max(0.0);
        vec![TempoEvent::Finished { phase: self.phase, start, end, duration: active, interruptions: self.interruptions, completed }]
    }

    fn begin(&mut self, p: TempoPhase, now: Date) {
        self.phase = p;
        self.started_at = Some(now);
        self.total = self.duration_of(p);
        self.ends_at = Some(add(now, self.total));
        self.paused_remaining = None;
        self.paused_at = None;
        self.paused_duration = 0.0;
        self.interruptions = 0;
    }
}

/// The text beside Kairos's tray icon, kept short.
pub mod menu_title {
    use super::*;
    pub const IDLE_LIMIT: usize = 20;
    pub const RUNNING_LIMIT: usize = 16;

    pub fn make(name: &str, phase: TempoPhase, remaining: f64, awaiting_focus: bool) -> String {
        if awaiting_focus {
            return "Break over".into();
        }
        match phase {
            TempoPhase::Idle => cap(name, IDLE_LIMIT),
            TempoPhase::Focus => format!("{} · {}", cap(name, RUNNING_LIMIT), clock(remaining)),
            _ => format!("Break · {}", clock(remaining)),
        }
    }

    pub fn cap(text: &str, limit: usize) -> String {
        if text.chars().count() <= limit {
            return text.to_string();
        }
        let head: String = text.chars().take(limit - 1).collect();
        format!("{}\u{2026}", head.trim())
    }
}

/// Time left as "m:ss", rounded up so a phase never shows 0:00 while it still runs.
pub fn clock(seconds: f64) -> String {
    let s = seconds.ceil().max(0.0) as i64;
    format!("{}:{:02}", s / 60, s % 60)
}
