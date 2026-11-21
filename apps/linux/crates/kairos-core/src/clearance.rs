//! Keeps other apps' windows from sitting under Kairos Bar while it is pinned open. The bar sets
//! a strut so maximised windows stop at it; this moves or shrinks normal windows that overlap
//! it. Coordinates are X11 root coordinates: top-left origin, y down.

use crate::models::DockEdge;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect { x, y, w, h }
    }
    pub fn max_x(&self) -> f64 {
        self.x + self.w
    }
    pub fn max_y(&self) -> f64 {
        self.y + self.h
    }
    pub fn mid_x(&self) -> f64 {
        self.x + self.w / 2.0
    }
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.max_x() && o.x < self.max_x() && self.y < o.max_y() && o.y < self.max_y()
    }
    pub fn intersection_area(&self, o: &Rect) -> f64 {
        let w = (self.max_x().min(o.max_x()) - self.x.max(o.x)).max(0.0);
        let h = (self.max_y().min(o.max_y()) - self.y.max(o.y)).max(0.0);
        w * h
    }
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.max_x() && py >= self.y && py < self.max_y()
    }
}

/// The strip the bar takes at rest: from the screen edge to the bar's inner edge.
pub fn strip(edge: DockEdge, panel: Rect, lead: f64, length: f64, depth: f64) -> Rect {
    match edge {
        DockEdge::Bottom => Rect::new(panel.x + lead, panel.max_y() - depth, length, depth),
        DockEdge::Left => Rect::new(panel.x, panel.y + lead, depth, length),
        DockEdge::Right => Rect::new(panel.max_x() - depth, panel.y + lead, depth, length),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub frame: Rect,
    pub can_move: bool,
    pub can_resize: bool,
    pub is_standard: bool,
    pub is_full_screen: bool,
    pub is_minimized: bool,
}

impl Window {
    pub fn new(frame: Rect) -> Self {
        Window { frame, can_move: true, can_resize: true, is_standard: true, is_full_screen: false, is_minimized: false }
    }
}

/// The frame `window` should have so it ends at the bar's inner edge, or None to leave it.
pub fn clear(window: &Window, strip: &Rect, edge: DockEdge, area: &Rect) -> Option<Rect> {
    let f = window.frame;
    if !window.is_standard || window.is_full_screen || window.is_minimized || f.w <= 0.0 || f.h <= 0.0 || !f.intersects(strip) {
        return None;
    }
    let mut t = f;
    match edge {
        DockEdge::Bottom => {
            let limit = strip.y;
            if f.h <= limit - area.y {
                t.y = f.y.min(limit - f.h);
            } else {
                t.y = f.y.max(area.y);
                t.h = limit - t.y;
            }
        }
        DockEdge::Left => {
            let limit = strip.max_x();
            if f.w <= area.max_x() - limit {
                t.x = f.x.max(limit);
            } else {
                t.x = limit;
                t.w = f.max_x().min(area.max_x()) - limit;
            }
        }
        DockEdge::Right => {
            let limit = strip.x;
            if f.w <= limit - area.x {
                t.x = f.x.min(limit - f.w);
            } else {
                t.x = f.x.max(area.x);
                t.w = limit - t.x;
            }
        }
    }
    if (t.x != f.x || t.y != f.y) && !window.can_move {
        return None;
    }
    if (t.w != f.w || t.h != f.h) && !window.can_resize {
        return None;
    }
    if t.w < 120.0 || t.h < 80.0 {
        return None;
    }
    if t == f {
        None
    } else {
        Some(t)
    }
}

/// The monitor that shows most of `window`.
pub fn display_showing(window: &Rect, displays: &[(usize, Rect)]) -> Option<usize> {
    displays
        .iter()
        .map(|(id, r)| (*id, r.intersection_area(window)))
        .filter(|(_, a)| *a > 0.0)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(id, _)| id)
}
