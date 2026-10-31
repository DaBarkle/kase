//! The tray quick look: a small island-styled card by the tray with the three rings, the
//! noise control modes and "Open Kase". It has its own layer surface (`main.rs`) that,
//! unlike the island, takes clicks. Only a tray click opens it, so it never shows over a
//! game by itself. Plain data here; `view.rs` draws it.

use crate::spring::{Feel, Spring};
use kase::overlay_protocol::{ListeningMode, OverlayPosition, OverlayState};

pub const W: f32 = 304.0;
/// Transparent room around the card inside its surface.
pub const PAD: f32 = 4.0;
pub const INSET: f32 = 20.0;
pub const RINGS_Y: f32 = 60.0;
pub const ROW_H: f32 = 32.0;
pub const MODES_Y: f32 = 108.0;
const BOTTOM: f32 = 16.0;

const OPEN: Feel = Feel::new(0.42, 0.22);
const CLOSE: Feel = Feel::new(0.24, 0.0);
const FADE: Feel = Feel::new(0.22, 0.0);
const GLIDE: Feel = Feel::new(0.3, 0.15);

/// Never touched: it goes away on its own after this long.
const UNTOUCHED: f32 = 6.0;
/// The pointer left it: it goes away after this long.
const AFTER_LEAVE: f32 = 1.2;

/// What the pointer is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Mode(usize),
    Open,
}

/// Where the "Open Kase" row starts.
pub fn button_y(state: &OverlayState) -> f32 {
    if state.modes.is_empty() {
        MODES_Y
    } else {
        MODES_Y + ROW_H + 12.0
    }
}

/// The card's size (without its padding).
pub fn size(state: &OverlayState) -> (f32, f32) {
    (W, button_y(state) + ROW_H + BOTTOM)
}

/// What sits under `(x, y)`, in card coordinates.
pub fn hit(state: &OverlayState, x: f32, y: f32) -> Option<Hit> {
    let inside_x = (INSET..W - INSET).contains(&x);
    let by = button_y(state);
    if inside_x && (by..by + ROW_H).contains(&y) {
        return Some(Hit::Open);
    }
    let n = state.modes.len();
    if n > 0 && inside_x && (MODES_Y..MODES_Y + ROW_H).contains(&y) {
        let seg = (W - 2.0 * INSET) / n as f32;
        return Some(Hit::Mode((((x - INSET) / seg) as usize).min(n - 1)));
    }
    None
}

#[derive(Debug, Clone)]
pub struct Quick {
    pub corner: OverlayPosition,
    /// 0 = folded into the corner, 1 = open.
    pub open: Spring,
    /// Seconds since it opened.
    pub age: f32,
    pub hover: Option<Hit>,
    entered: bool,
    /// Seconds since the pointer left, while it is outside.
    away: Option<f32>,
    /// The highlighted mode's position (index), gliding between modes.
    pub selected: Spring,
    /// A mode just tapped, shown until the AirPods confirm it.
    pub pending: Option<ListeningMode>,
    reduced: bool,
}

impl Quick {
    pub fn new(corner: OverlayPosition, reduced: bool) -> Self {
        let mut open = Spring::new(0.0, OPEN, 0.002);
        open.to(1.0, if reduced { FADE } else { OPEN });
        Self {
            corner,
            open,
            age: 0.0,
            hover: None,
            entered: false,
            away: None,
            selected: Spring::new(-1.0, GLIDE, 0.002),
            pending: None,
            reduced,
        }
    }

    /// Scale and opacity of the card now. With Reduce motion it only fades.
    pub fn presence(&self) -> (f32, f32) {
        let v = self.open.value;
        let scale = if self.reduced { 1.0 } else { 0.6 + 0.4 * v };
        (scale, v.clamp(0.0, 1.0))
    }

    pub fn reopen(&mut self) {
        self.open.to(1.0, if self.reduced { FADE } else { OPEN });
        self.entered = false;
        self.away = None;
        self.age = 0.0;
    }

    pub fn close(&mut self) {
        self.open.to(0.0, if self.reduced { FADE } else { CLOSE });
    }

    pub fn closing(&self) -> bool {
        self.open.target == 0.0
    }

    /// Fully folded away: its surface can go.
    pub fn gone(&self) -> bool {
        self.closing() && self.open.value <= 0.02
    }

    pub fn pointer(&mut self, hit: Option<Hit>, inside: bool) {
        self.hover = hit;
        if inside {
            self.entered = true;
            self.away = None;
        } else if self.away.is_none() {
            self.away = Some(0.0);
        }
    }

    /// Follows the AirPods' mode; a tapped mode wins until they report it.
    pub fn follow(&mut self, state: &OverlayState) {
        if self.pending.is_some() && self.pending == state.listening_mode {
            self.pending = None;
        }
        let mode = self.pending.or(state.listening_mode);
        let at = mode.and_then(|m| state.modes.iter().position(|x| *x == m));
        match at {
            Some(i) if self.selected.value < 0.0 || self.reduced => self.selected.snap(i as f32),
            Some(i) => self.selected.to(i as f32, GLIDE),
            None => self.selected.snap(-1.0),
        }
    }

    /// Advances by `dt` (its timers in full; the springs at most 50 ms, so a slow tick
    /// never skips their motion). Returns true while it still moves (frames needed).
    pub fn step(&mut self, dt: f32) -> bool {
        self.age += dt;
        if let Some(a) = &mut self.away {
            *a += dt;
        }
        let leave = match self.away {
            Some(a) => self.entered && a >= AFTER_LEAVE,
            None => false,
        };
        if !self.closing() && (leave || (!self.entered && self.age >= UNTOUCHED)) {
            self.close();
        }
        let spring_dt = dt.min(0.05);
        self.open.step(spring_dt) | self.selected.step(spring_dt)
    }
}

#[cfg(test)]
#[path = "quick_tests.rs"]
mod tests;
