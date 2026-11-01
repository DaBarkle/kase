//! Gathers a burst of AirPods updates into one moment before the island reacts.
//!
//! One thing a person does arrives as a burst: an ear change, then a battery update
//! 46-113 ms later, then the same values repeated for up to 1.5 s (measured on AirPods 4,
//! 2026-11-01, see docs/JOURNEYS.md). Reacting to each packet makes the island jump.
//! The settler says when the burst is over: 300 ms after the last change that matters to
//! the island, and never more than 1.5 s after the first, so nothing waits long.
//! Repeats and level changes inside a battery band don't count; they still show at once
//! (the rings update in place), they just don't start a new moment.

use std::time::{Duration, Instant};

use kase::overlay_protocol::{ComponentState, OverlayState};

/// Quiet time after the last meaningful change.
pub const QUIET: Duration = Duration::from_millis(300);
/// The longest a burst can hold a moment back.
pub const MAX_WAIT: Duration = Duration::from_millis(1500);

/// Battery band: 0 = 10% or less, 1 = 20% or less, 2 = above.
pub fn band(level: u8) -> u8 {
    match level {
        0..=10 => 0,
        11..=20 => 1,
        _ => 2,
    }
}

fn part_key(c: &Option<ComponentState>) -> Option<(bool, bool, bool, bool, u8)> {
    c.as_ref()
        .map(|c| (c.present, c.charging, c.out_of_ear, c.stale, band(c.level)))
}

/// The two states differ in something the island would tell the person about.
pub fn meaningful(a: Option<&OverlayState>, b: &OverlayState) -> bool {
    let Some(a) = a else { return true };
    a.connected != b.connected
        || a.elsewhere != b.elsewhere
        || a.in_case != b.in_case
        || a.mic != b.mic
        || a.mic_failures != b.mic_failures
        || a.listening_mode != b.listening_mode
        || part_key(&a.left) != part_key(&b.left)
        || part_key(&a.right) != part_key(&b.right)
        || part_key(&a.case) != part_key(&b.case)
        || part_key(&a.headphone) != part_key(&b.headphone)
}

#[derive(Debug, Default)]
pub struct Settler {
    first: Option<Instant>,
    due: Option<Instant>,
}

impl Settler {
    /// A meaningful change arrived at `now`.
    pub fn changed(&mut self, now: Instant) {
        let first = *self.first.get_or_insert(now);
        self.due = Some((now + QUIET).min(first + MAX_WAIT));
    }

    /// When the current burst is over (None when there is none).
    pub fn due(&self) -> Option<Instant> {
        self.due
    }

    /// True once, when the burst is over at `now`.
    pub fn take(&mut self, now: Instant) -> bool {
        if self.due.is_some_and(|t| now >= t) {
            *self = Self::default();
            return true;
        }
        false
    }

    /// Ends the burst now (tests, and commands that must see the latest state).
    pub fn flush(&mut self) -> bool {
        let was = self.due.is_some();
        *self = Self::default();
        was
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buds() -> OverlayState {
        let c = |level| {
            Some(ComponentState {
                level,
                present: true,
                ..Default::default()
            })
        };
        OverlayState {
            connected: true,
            left: c(80),
            right: c(75),
            case: c(40),
            ..Default::default()
        }
    }

    #[test]
    fn repeats_and_small_level_changes_are_not_news() {
        let a = buds();
        assert!(!meaningful(Some(&a), &a.clone()));
        let mut b = a.clone();
        b.left.as_mut().unwrap().level = 79;
        assert!(!meaningful(Some(&a), &b), "same band");
        b.left.as_mut().unwrap().level = 20;
        assert!(meaningful(Some(&a), &b), "crossed into the low band");
        let mut c = a.clone();
        c.right.as_mut().unwrap().out_of_ear = true;
        assert!(meaningful(Some(&a), &c));
        assert!(meaningful(None, &a));
    }

    #[test]
    fn settles_after_quiet_time_but_never_waits_long() {
        let t0 = Instant::now();
        let mut s = Settler::default();
        assert!(!s.take(t0));
        s.changed(t0);
        assert_eq!(s.due(), Some(t0 + QUIET));
        s.changed(t0 + Duration::from_millis(100));
        assert!(!s.take(t0 + QUIET), "a later change pushes it back");
        assert!(s.take(t0 + Duration::from_millis(400)));
        assert!(!s.take(t0 + Duration::from_millis(500)), "only once");

        // A long burst: a change every 200 ms for 3 s still settles at 1.5 s.
        let mut s = Settler::default();
        let mut fired = None;
        for i in 0..15u64 {
            let t = t0 + Duration::from_millis(200 * i);
            if s.take(t) {
                fired = Some(t);
                break;
            }
            s.changed(t);
        }
        assert_eq!(fired, Some(t0 + Duration::from_millis(1600)));
    }
}
