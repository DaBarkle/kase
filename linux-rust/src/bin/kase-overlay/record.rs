//! `--script FILE`: plays a scripted journey on the test clock and records every frame
//! (see `kase::testclock` and `tools/visual-qa/journeys/`).
//!
//! A script is one JSON object:
//!
//! ```json
//! {
//!   "config": { "mode": "compact" },
//!   "state":  { "name": "AirPods 4", "kind": "airpods4", "connected": true, ... },
//!   "events": [
//!     { "t_ms": 0,    "state": { "right": { "present": false } } },
//!     { "t_ms": 1400, "cmd": { "type": "show", "reason": "test" } }
//!   ],
//!   "end_ms": 9000
//! }
//! ```
//!
//! `config` patches the default overlay config and is applied before the first frame.
//! Each event's `state` is a patch over the state so far (objects merge key by key,
//! `null` removes a key) and is sent as a `state` command; `cmd` is any protocol command.
//! Without `end_ms` the run ends once the last event has played and the island is at
//! rest with nothing scheduled.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use kase::overlay_protocol::{OverlayCommand, OverlayConfig, OverlayState};
use kase::testclock;
use serde_json::Value;

/// A surface to capture this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Island,
    Quick,
}

impl Surface {
    pub fn tag(self) -> &'static str {
        match self {
            Surface::Island => "",
            Surface::Quick => "quick",
        }
    }
}

pub struct Recorder {
    pub config: OverlayConfig,
    /// Commands still to come, by test time in ms.
    events: VecDeque<(f64, OverlayCommand)>,
    end_ms: Option<f64>,
    /// Bumped when the watchdog gives up on a capture, so a late result is ignored.
    pub generation: u64,
    /// Surfaces still to capture this frame, and the files saved so far.
    pub pending: Vec<(Surface, iced::window::Id)>,
    pub saved: Vec<(Surface, String)>,
    /// Wall-clock time of the last step forward (for the watchdog).
    pub progress: Instant,
    /// The starting state, played before time zero until the island is at rest, so a
    /// journey starts from a settled island (`"start": "settled"`, the default).
    pub warmup: Option<OverlayState>,
}

impl Recorder {
    pub fn load(path: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        Self::parse(&text).map_err(|e| format!("{path}: {e}"))
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let script: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let mut config =
            serde_json::to_value(OverlayConfig::default()).map_err(|e| e.to_string())?;
        if let Some(patch) = script.get("config") {
            testclock::merge(&mut config, patch);
        }
        let config: OverlayConfig =
            serde_json::from_value(config).map_err(|e| format!("config: {e}"))?;
        let mut state = script
            .get("state")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));
        let settled = script.get("start").and_then(Value::as_str) != Some("cold");
        let warmup = match script.get("state") {
            Some(_) if settled => {
                Some(serde_json::from_value(state.clone()).map_err(|e| format!("state: {e}"))?)
            }
            _ => None,
        };
        let mut events = vec![];
        for (i, ev) in script
            .get("events")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let t = ev.get("t_ms").and_then(Value::as_f64).unwrap_or(0.0);
            if let Some(patch) = ev.get("state") {
                testclock::merge(&mut state, patch);
                let s: OverlayState = serde_json::from_value(state.clone())
                    .map_err(|e| format!("event {i} state: {e}"))?;
                events.push((t, OverlayCommand::State { state: s }));
            }
            if let Some(cmd) = ev.get("cmd") {
                let c: OverlayCommand = serde_json::from_value(cmd.clone())
                    .map_err(|e| format!("event {i} cmd: {e}"))?;
                events.push((t, c));
            }
        }
        // Stable: same-time events keep their order.
        events.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(Self {
            config,
            events: events.into(),
            end_ms: script.get("end_ms").and_then(Value::as_f64),
            generation: 0,
            pending: vec![],
            saved: vec![],
            progress: Instant::now(),
            warmup,
        })
    }

    /// The commands due by test time `ms`.
    pub fn due(&mut self, ms: f64) -> Vec<OverlayCommand> {
        let mut out = vec![];
        while self.events.front().is_some_and(|(t, _)| *t <= ms + 1e-6) {
            if let Some((_, c)) = self.events.pop_front() {
                out.push(c);
            }
        }
        out
    }

    /// When the next command is due (test time).
    pub fn next_event(&self, now: Instant) -> Option<Instant> {
        let base = now - Duration::from_secs_f64(testclock::elapsed_ms() / 1000.0);
        self.events
            .front()
            .map(|(t, _)| base + Duration::from_secs_f64(t / 1000.0))
    }

    pub fn end_at(&self, now: Instant) -> Option<Instant> {
        let base = now - Duration::from_secs_f64(testclock::elapsed_ms() / 1000.0);
        self.end_ms
            .map(|t| base + Duration::from_secs_f64(t / 1000.0))
    }

    /// Every event has played.
    pub fn played(&self) -> bool {
        self.events.is_empty()
    }

    pub fn has_end(&self) -> bool {
        self.end_ms.is_some()
    }

    pub fn past_end(&self, ms: f64) -> bool {
        self.end_ms.is_some_and(|e| ms >= e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_patches_accumulate_and_sort() {
        let r = Recorder::parse(
            r#"{
              "config": {"mode": "compact"},
              "state": {"name": "AirPods 4", "kind": "airpods4", "connected": true,
                        "left": {"level": 80, "present": true}},
              "events": [
                {"t_ms": 500, "state": {"left": {"present": false}}},
                {"t_ms": 0, "cmd": {"type": "show", "reason": "t"}}
              ],
              "end_ms": 2000
            }"#,
        )
        .expect("parses");
        assert_eq!(r.config.mode, kase::overlay_protocol::OverlayMode::Compact);
        let ev: Vec<_> = r.events.iter().collect();
        assert_eq!(ev.len(), 2);
        assert!(matches!(ev[0].1, OverlayCommand::Show { .. }));
        let OverlayCommand::State { state } = &ev[1].1 else {
            panic!("state second")
        };
        let left = state.left.as_ref().expect("left kept");
        assert_eq!(left.level, 80);
        assert!(!left.present);
        assert!(r.past_end(2000.0) && !r.past_end(1999.0));
    }

    #[test]
    fn due_pops_in_order() {
        let mut r = Recorder::parse(
            r#"{"events": [{"t_ms": 0, "cmd": {"type": "hide"}},
                           {"t_ms": 10, "cmd": {"type": "hide"}}]}"#,
        )
        .expect("parses");
        assert_eq!(r.due(0.0).len(), 1);
        assert_eq!(r.due(5.0).len(), 0);
        assert_eq!(r.due(10.0).len(), 1);
        assert!(r.played());
    }

    #[test]
    fn every_journey_parses() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tools/visual-qa/journeys");
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") {
                Recorder::load(&p.to_string_lossy()).unwrap_or_else(|e| panic!("{e}"));
            }
        }
    }
}
