//! Spots a control channel the AirPods never talk on, and opens a fresh one.
//!
//! Now and then (13 of 75 connects on David's PC, 2026-10-29 to 2026-11-02) the L2CAP
//! control channel opens but the AirPods never send a single packet on it: no battery, no
//! ear report, no reply to the setup. Audio plays as normal, so it looks connected, but the
//! stem claim never lands and holding the left stem changes noise control instead of
//! swapping mics. Sending the setup again on the same channel never helped (10 tries over
//! two sessions on 2026-11-01). The channel itself is dead, so it is closed and a new one
//! opened, as many times as it takes while the Bluetooth link is up.
//!
//! Everything here is pure so the timings are covered by tests.

use std::time::Duration;

/// A channel that has said nothing for this long since it opened gets the setup again
/// (cheap, and enough when the AirPods were only slow).
pub(super) const RESEND_SETUP_AFTER: Duration = Duration::from_secs(4);
/// A channel that has still said nothing this long after it opened is dead: open a new
/// one. Every good channel in the logs spoke within 9 s (most within 1 s), and 12 s
/// stays clear of the 20 s stuck-session watchdog.
pub(super) const REOPEN_AFTER: Duration = Duration::from_secs(12);
/// Waits before opening a new channel after one in a row came up silent. In the case
/// with the lid open the AirPods may stay quiet for minutes, so the waits grow, but never
/// past 30 s: a pod taken out and put in an ear has its stem working again soon after.
const REOPEN_DELAYS: [u64; 5] = [1, 2, 5, 10, 30];

/// What to do about a channel this health check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SilenceStep {
    /// The AirPods have talked, or it is too soon to tell.
    Wait,
    /// Send the setup once more on this channel.
    ResendSetup,
    /// Close this channel and open a new one.
    Reopen,
}

/// Decides from whether anything came in since the channel opened, how long ago that was,
/// and whether the setup has been sent again already.
pub(super) fn decide(heard: bool, since_open: Duration, resent: bool) -> SilenceStep {
    if heard {
        SilenceStep::Wait
    } else if since_open >= REOPEN_AFTER {
        SilenceStep::Reopen
    } else if !resent && since_open >= RESEND_SETUP_AFTER {
        SilenceStep::ResendSetup
    } else {
        SilenceStep::Wait
    }
}

/// How long to wait before opening the next channel, after `silent_in_a_row` channels in
/// a row came up silent (1 for the first).
pub(super) fn reopen_delay(silent_in_a_row: u32) -> Duration {
    let i = (silent_in_a_row.max(1) - 1) as usize;
    Duration::from_secs(REOPEN_DELAYS[i.min(REOPEN_DELAYS.len() - 1)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    #[test]
    fn a_channel_that_talks_is_never_reopened() {
        for secs in [0, 4, 12, 60, 3600] {
            assert_eq!(decide(true, s(secs), false), SilenceStep::Wait);
            assert_eq!(decide(true, s(secs), true), SilenceStep::Wait);
        }
    }

    #[test]
    fn a_new_channel_gets_a_moment_to_speak() {
        assert_eq!(decide(false, s(0), false), SilenceStep::Wait);
        assert_eq!(decide(false, s(3), false), SilenceStep::Wait);
    }

    #[test]
    fn a_quiet_channel_gets_the_setup_again_once() {
        assert_eq!(decide(false, s(4), false), SilenceStep::ResendSetup);
        assert_eq!(decide(false, s(6), true), SilenceStep::Wait);
        assert_eq!(decide(false, s(11), true), SilenceStep::Wait);
    }

    /// 2026-11-01 22:50 and 23:51 on David's PC: the AirPods said nothing for 12 minutes
    /// and more, five setup resends went unanswered, and the left hold changed noise
    /// control. The channel must be replaced, not kept with more resends.
    #[test]
    fn a_silent_channel_is_replaced_not_kept() {
        assert_eq!(decide(false, s(12), true), SilenceStep::Reopen);
        assert_eq!(decide(false, s(12), false), SilenceStep::Reopen);
        assert_eq!(decide(false, s(720), true), SilenceStep::Reopen);
    }

    #[test]
    fn reopening_is_decided_before_the_stuck_watchdog_fires() {
        assert!(REOPEN_AFTER < super::super::SESSION_STUCK);
        assert!(RESEND_SETUP_AFTER < REOPEN_AFTER);
    }

    #[test]
    fn silent_channels_are_retried_forever_with_capped_waits() {
        assert_eq!(reopen_delay(0), s(1));
        assert_eq!(reopen_delay(1), s(1));
        assert_eq!(reopen_delay(2), s(2));
        assert_eq!(reopen_delay(3), s(5));
        assert_eq!(reopen_delay(4), s(10));
        assert_eq!(reopen_delay(5), s(30));
        assert_eq!(reopen_delay(500), s(30));
    }
}
