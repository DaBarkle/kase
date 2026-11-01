//! Where each pod is (in the case or out) and whether the lid is open, as left/right
//! truth for the live case animation.
//!
//! Two sources feed it:
//!
//! * **AACP ear reports** (0x06), fast and exact while the control channel is up. They
//!   name the pods "primary, secondary", not left/right, and the primary swaps when a pod
//!   goes into the case. The battery report (0x04) lists the primary pod first, so its
//!   order tells which is which (checked against the pods' Charging flags in 166 of 166
//!   reports on David's PC). An ear report that lands before the battery report of a swap
//!   is mapped whichever way moves fewer pods, then corrected by the battery order.
//! * **BLE adverts**, which also work before the control channel is up (the first second
//!   after the lid opens) and when the AirPods send this PC no ear reports. They lose to
//!   a recent ear report, because they can lag a real change.
//!
//! The lid is read from both: the advert's lid bit, and from the ear reports themselves:
//! a pod in the case that turns "disconnected" means the lid just shut on it, the reverse
//! means it opened, and a pod can only come out of or go into an open case.

use crate::bluetooth::aacp::{BatteryComponent, BatteryInfo, BatteryStatus, EarDetectionStatus};
use crate::core::battery::BleBatteryReading;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// After an ear report, adverts can still say the old thing for a moment.
pub const ADVERT_HOLD_OFF: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Lid {
    #[default]
    Unknown,
    Open,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Pod {
    #[default]
    Unknown,
    /// In its well in the case.
    Seated,
    /// Out of the case (in an ear or in a hand).
    Out,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CaseSource {
    #[default]
    None,
    Aacp,
    Advert,
    Link,
}

/// What the case animation should show. `seq` goes up on every change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CaseState {
    pub lid: Lid,
    pub left: Pod,
    pub right: Pod,
    /// Where the last change came from.
    pub source: CaseSource,
    pub seq: u64,
}

impl CaseState {
    pub fn describe(&self) -> String {
        format!(
            "lid {:?}, L {:?}, R {:?} ({:?})",
            self.lid, self.left, self.right, self.source
        )
    }
}

#[derive(Debug, Default)]
pub struct CaseTracker {
    state: CaseState,
    /// Which pod is primary in the ear reports, from the battery order.
    primary_left: Option<bool>,
    /// Last ear report (primary, secondary), kept to re-map it when the order changes.
    last_ear: Option<(EarDetectionStatus, EarDetectionStatus)>,
    /// What each pod last reported, by side, before mapping to `Pod`.
    raw: (Option<EarDetectionStatus>, Option<EarDetectionStatus>),
    last_aacp: Option<Instant>,
}

fn place(s: EarDetectionStatus, was: Pod) -> Pod {
    match s {
        EarDetectionStatus::InCase => Pod::Seated,
        EarDetectionStatus::InEar | EarDetectionStatus::OutOfEar => Pod::Out,
        // A pod in a shut case drops off; one that was out and drops is out of range.
        EarDetectionStatus::Disconnected => match was {
            Pod::Seated => Pod::Seated,
            _ => Pod::Unknown,
        },
    }
}

impl CaseTracker {
    pub fn state(&self) -> CaseState {
        self.state
    }

    /// Takes the order of a battery report. Returns true when the state changed.
    pub fn on_battery(&mut self, info: &[BatteryInfo], now: Instant) -> bool {
        let first = info.iter().find_map(|i| match i.component {
            BatteryComponent::Left => Some(true),
            BatteryComponent::Right => Some(false),
            _ => None,
        });
        let Some(first) = first else { return false };
        if self.last_ear.is_none() {
            // No ear report yet on this connection (it often starts with the battery, and
            // some connections send no ear reports at all): a pod that charges is in the case.
            self.primary_left = Some(first);
            return self.apply_charging(info, now);
        }
        if self.primary_left == Some(first) {
            return false;
        }
        self.primary_left = Some(first);
        match self.last_ear {
            Some((p, s)) => self.apply_ear(p, s, now),
            None => false,
        }
    }

    fn apply_charging(&mut self, info: &[BatteryInfo], now: Instant) -> bool {
        let mut next = self.state;
        for i in info {
            let pod = match i.status {
                BatteryStatus::Charging => Pod::Seated,
                BatteryStatus::NotCharging => Pod::Out,
                _ => continue,
            };
            match i.component {
                BatteryComponent::Left => next.left = pod,
                BatteryComponent::Right => next.right = pod,
                _ => {}
            }
        }
        let moved = |was: Pod, now_pod: Pod| was != Pod::Unknown && was != now_pod;
        if moved(self.state.left, next.left) || moved(self.state.right, next.right) {
            // Pods can only come out of or go into an open case.
            next.lid = Lid::Open;
        }
        self.last_aacp = Some(now);
        self.commit(next, CaseSource::Aacp)
    }

    /// Takes an AACP ear report (primary, secondary). Returns true when the state changed.
    pub fn on_ear(
        &mut self,
        primary: EarDetectionStatus,
        secondary: EarDetectionStatus,
        now: Instant,
    ) -> bool {
        self.last_aacp = Some(now);
        self.last_ear = Some((primary, secondary));
        let Some(keep) = self.primary_left else {
            if primary == secondary {
                return self.apply_sides(primary, secondary, now);
            }
            // Wait for the battery report to say which pod is which.
            return false;
        };
        // A swapped primary shows up as the old report reversed: pick whichever mapping
        // moves fewer pods; the battery report that follows confirms or corrects it.
        let moves = |left_first: bool| {
            let (l, r) = if left_first {
                (primary, secondary)
            } else {
                (secondary, primary)
            };
            usize::from(self.raw.0.is_some_and(|o| o != l))
                + usize::from(self.raw.1.is_some_and(|o| o != r))
        };
        if moves(!keep) < moves(keep) {
            self.primary_left = Some(!keep);
        }
        self.apply_ear(primary, secondary, now)
    }

    fn apply_ear(
        &mut self,
        primary: EarDetectionStatus,
        secondary: EarDetectionStatus,
        now: Instant,
    ) -> bool {
        let (l, r) = if self.primary_left.unwrap_or(true) {
            (primary, secondary)
        } else {
            (secondary, primary)
        };
        self.apply_sides(l, r, now)
    }

    fn apply_sides(&mut self, l: EarDetectionStatus, r: EarDetectionStatus, _now: Instant) -> bool {
        use EarDetectionStatus::{Disconnected, InCase};
        let (old_l, old_r) = self.raw;
        let mut next = self.state;
        next.left = place(l, self.state.left);
        next.right = place(r, self.state.right);
        let mut lid = next.lid;
        for (old, new, was, now_pod) in [
            (old_l, l, self.state.left, next.left),
            (old_r, r, self.state.right, next.right),
        ] {
            match (old, new) {
                (Some(InCase), Disconnected) => lid = Lid::Closed,
                (Some(Disconnected), InCase) => lid = Lid::Open,
                _ if was != Pod::Unknown && now_pod != Pod::Unknown && was != now_pod => {
                    lid = Lid::Open
                }
                _ => {}
            }
        }
        next.lid = lid;
        self.raw = (Some(l), Some(r));
        self.commit(next, CaseSource::Aacp)
    }

    /// Takes a decoded advert. Ignored for pods while a recent ear report is in charge.
    pub fn on_advert(&mut self, reading: &BleBatteryReading, now: Instant) -> bool {
        let mut next = self.state;
        if let Some(open) = reading.lid_open {
            next.lid = if open { Lid::Open } else { Lid::Closed };
        }
        let aacp_fresh = self
            .last_aacp
            .is_some_and(|t| now.saturating_duration_since(t) < ADVERT_HOLD_OFF);
        if !aacp_fresh {
            let pod = |in_case: bool| if in_case { Pod::Seated } else { Pod::Out };
            next.left = pod(reading.left_in_case);
            next.right = pod(reading.right_in_case);
        } else if reading.lid_open.is_some()
            && next.lid != self.state.lid
            && self.state.lid != Lid::Unknown
        {
            // An ear report just spoke; trust it about the lid too.
            next.lid = self.state.lid;
        }
        self.commit(next, CaseSource::Advert)
    }

    /// The Bluetooth link to this PC is gone.
    pub fn on_link_lost(&mut self) -> bool {
        self.last_aacp = None;
        self.last_ear = None;
        self.raw = (None, None);
        let mut next = self.state;
        if next.left == Pod::Seated && next.right == Pod::Seated {
            // Both in the case and the link dropped: the lid is shut.
            next.lid = Lid::Closed;
        } else {
            next.left = if next.left == Pod::Seated {
                Pod::Seated
            } else {
                Pod::Unknown
            };
            next.right = if next.right == Pod::Seated {
                Pod::Seated
            } else {
                Pod::Unknown
            };
        }
        self.commit(next, CaseSource::Link)
    }

    fn commit(&mut self, mut next: CaseState, source: CaseSource) -> bool {
        next.source = self.state.source;
        next.seq = self.state.seq;
        if next == self.state {
            return false;
        }
        next.source = source;
        next.seq = self.state.seq + 1;
        self.state = next;
        true
    }
}

#[cfg(test)]
#[path = "case_state_tests.rs"]
mod tests;
